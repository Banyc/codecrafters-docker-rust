use std::os::unix::process::CommandExt;

pub mod exec;
pub mod ls;
#[cfg(target_os = "linux")]
pub mod mounting;
pub mod pull_image;
pub mod rm;
pub mod rmi;
pub mod run;
pub mod token_auth;
pub mod www_authenticate;

static BASE_DIR: once_cell::sync::Lazy<std::path::PathBuf> =
    once_cell::sync::Lazy::new(|| std::path::PathBuf::from("/tmp/mydocker"));
static CONTAINERS: once_cell::sync::Lazy<std::path::PathBuf> =
    once_cell::sync::Lazy::new(|| BASE_DIR.join("containers"));
static PACKED_LAYER_DIR: once_cell::sync::Lazy<std::path::PathBuf> =
    once_cell::sync::Lazy::new(|| BASE_DIR.join("layers"));

fn container_dir(name: &str) -> std::path::PathBuf {
    CONTAINERS.join(name)
}

fn pid_file_path(name: &str) -> std::path::PathBuf {
    CONTAINERS.join(name).join("pid")
}

fn root_fs_path(name: &str) -> std::path::PathBuf {
    CONTAINERS.join(name).join("rootfs")
}

fn overlay_layer_dir(name: &str) -> std::path::PathBuf {
    CONTAINERS.join(name).join("layers")
}

#[allow(dead_code)]
fn overlay_fs_writable_layers_dir(name: &str) -> std::path::PathBuf {
    overlay_layer_dir(name).join("writable")
}

#[allow(dead_code)]
fn overlay_fs_work_dir(name: &str) -> std::path::PathBuf {
    overlay_fs_writable_layers_dir(name).join("work")
}

#[allow(dead_code)]
fn overlay_fs_upper_dir(name: &str) -> std::path::PathBuf {
    overlay_fs_writable_layers_dir(name).join("upper")
}

#[allow(dead_code)]
fn overlay_fs_lower_dir(name: &str) -> std::path::PathBuf {
    overlay_layer_dir(name).join("lower")
}

fn read_pid(pid_file_path: impl AsRef<std::path::Path>) -> Option<usize> {
    if !pid_file_path.as_ref().exists() {
        return None;
    }

    use std::io::Read;

    let file = std::fs::File::options()
        .read(true)
        .open(&pid_file_path)
        .unwrap();
    let mut file = std::io::BufReader::new(file);
    let mut pid = String::new();
    file.read_to_string(&mut pid).unwrap();
    let pid: usize = pid.parse().unwrap();
    Some(pid)
}

fn write_pid(pid_file_path: impl AsRef<std::path::Path>) {
    use std::io::Write;

    let pid = std::process::id();
    let _ = std::fs::remove_file(&pid_file_path);
    let mut file = std::fs::File::options()
        .create(true)
        .write(true)
        .open(&pid_file_path)
        .unwrap();
    file.write_all(format!("{pid}").as_bytes()).unwrap();
}

fn execute_command(
    command: impl AsRef<std::path::Path> + std::fmt::Debug,
    command_args: &[String],
    root: impl AsRef<std::path::Path>,
) -> anyhow::Result<()> {
    use anyhow::Context;

    // Chroot the root directory
    std::os::unix::fs::chroot(root).unwrap();
    std::env::set_current_dir("/").unwrap();

    #[cfg(target_os = "linux")]
    {
        let res = unsafe { libc::unshare(libc::CLONE_NEWNS) };
        if res != 0 {
            std::process::exit(res);
        }

        // The calling process is not moved into the new namespace.
        // The first child created by the calling process will have the process ID 1 and will assume the role of init(1) in the new namespace.
        let res = unsafe { libc::unshare(libc::CLONE_NEWPID) };
        if res != 0 {
            std::process::exit(res);
        }
    }

    // Execute the command
    let mut command_exec = std::process::Command::new(command.as_ref());
    command_exec
        .args(command_args)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit());
    unsafe {
        command_exec.pre_exec(move || {
            #[cfg(target_os = "linux")]
            {
                // Make sure child gets killed if parent dies
                let res = libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGHUP);
                if res != 0 {
                    return Err(nix::Error::from_i32(res).into());
                }

                mounting::mount_proc_in_container()?;
            }
            Ok(())
        });
    }
    let child = command_exec.spawn().with_context(|| {
        format!(
            "Tried to spawn '{:?}' with arguments {:?}",
            command, command_args
        )
    })?;
    let mut child = ChildGuard(child);

    // Wait for the child to exit
    let exit_status = child.get_mut().wait().with_context(|| {
        format!(
            "Tried to wait '{:?}' with arguments {:?}",
            command, command_args
        )
    })?;

    // Return exit code
    if let Some(code) = exit_status.code() {
        std::process::exit(code);
    }
    Ok(())
}

// https://stackoverflow.com/a/30540177/9920172
struct ChildGuard(std::process::Child);

impl ChildGuard {
    pub fn get_mut(&mut self) -> &mut std::process::Child {
        &mut self.0
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        // You can check std::thread::panicking() here
        if let Err(e) = self.0.kill() {
            println!("Could not kill child process `{}`: {e}", self.0.id());
        }
    }
}

fn process_alive(pid: usize) -> bool {
    let res = unsafe { libc::kill(pid as i32, 0) };
    res == 0
}
