use std::cell::{LazyCell, RefCell};
use std::fmt::Display;
use std::fs::{read_to_string, write};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::process::{self, Output};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{env, fs};

use anyhow::{Context, Result};
use chrono::{Days, Local, Timelike};
use elevated_command::Command as SudoCommand;
use serde::Deserialize;
use std::process::Command as StdCommand;

fn main() {
    // Step 1: Check if the current process is elevated.
    if !SudoCommand::is_elevated() {
        println!("Process is not elevated. Attempting to relaunch with elevated privileges.");

        // Step 2: Get the current executable.
        let exe_path =
            env::current_exe().expect("failed to determine the path of the current executable");
        // Collect any command-line arguments (skip the first, which is the exe path itself)
        let args: Vec<String> = env::args().skip(1).collect();

        // Step 3: Create command to run the current executable
        let mut cmd = StdCommand::new(exe_path);
        cmd.args(args);

        // Step 3: Relaunch the same executable with elevated privileges.
        let elevated = SudoCommand::new(cmd);

        // Execute the elevated process and wait for it to complete.
        match elevated.output() {
            Ok(output) => {
                print!("Elevated process exited with {}", output.status);
                match output.status.code() {
                    Some(n) if n > 32 => {
                        println!(" (Success)");
                        println!(
                            "Note that the process is no longer a child of this current process."
                        );
                        println!("The current process will exit but the elevated process will continue in the background,");
                        println!("silently sending messages to the log file. The only way to kill it is through Task Manager.");
                        process::exit(0);
                    }
                    Some(n) => {
                        process::exit(n);
                    }
                    None => {
                        process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to launch elevated process: {}", e);
                process::exit(1);
            }
        }
    }

    // At this point, the process is elevated.
    log_to_file("Process is running with elevated privileges.").unwrap();

    if let Err(e) = elevated_main() {
        show_dialog(format!("Downtime encountered an error:\n{:#}", e)).unwrap();
        process::exit(1);
    }
}

// This is separate so the error can be printed to the screen and also logged to the log file
fn elevated_main() -> Result<()> {
    let config = read_config()?;

    let mut enabled = read_state()?;

    loop {
        let until_next = until_next_sleep(!enabled, &config)?;
        sleep(until_next);

        log_to_file("woke up!")?;
        if enabled {
            show_dialog("Downtime is over, you can access your sites now")?;
            restore_hosts()?;
        } else {
            show_dialog("Downtime is now active, your browser will be closed")?;
            write_to_hosts(&config.blocked_sites)?;
            kill_browser(&config.browser_exe)?;
        }

        enabled = !enabled;
        save_state(enabled)?;
    }

    #[allow(unreachable_code)]
    Ok(())
}

fn show_dialog(message: impl Display) -> Result<()> {
    use std::ptr::null_mut as NULL;
    use winapi::um::winuser;

    let l_msg: Vec<u16> = format!("{}\0", message).encode_utf16().collect();
    let l_title: Vec<u16> = "Downtime for Windows\0".encode_utf16().collect();

    unsafe {
        winuser::MessageBoxW(
            NULL(),
            l_msg.as_ptr(),
            l_title.as_ptr(),
            winuser::MB_OK | winuser::MB_ICONINFORMATION,
        );
    }

    log_to_file(message)?;

    Ok(())
}

/// if `until_enable` is true, this will return the instant of enable
/// if `until_enable` is false, this will return the instant of disable
fn until_next_sleep(until_enable: bool, config: &Config) -> Result<Duration> {
    let now = Local::now();

    let hour_and_minute = if until_enable {
        &config.enable_downtime
    } else {
        &config.disable_downtime
    };

    let config_name = if until_enable {
        "enable-downtime"
    } else {
        "disable-downtime"
    };

    let mut sleep_time = now
        .with_hour(if hour_and_minute.hour == 24 {
            0
        } else {
            hour_and_minute.hour
        })
        .with_context(|| format!("{config_name}.hour is invalid"))?
        .with_minute(hour_and_minute.minute)
        .with_context(|| format!("{config_name}.minute is invalid"))?
        .with_second(0)
        .context("0 should not be an invalid second")?;

    log_to_file(format!("now: {now}, sleep time: {sleep_time}"))?;

    let mut duration = sleep_time.signed_duration_since(now);

    if duration.num_seconds().is_negative() {
        sleep_time = sleep_time.checked_add_days(Days::new(1)).unwrap();
        duration = sleep_time.signed_duration_since(now)
    }

    log_to_file(format!(
        "duration until: {:.2} hrs",
        duration.num_seconds() as f64 / 60.0 / 60.0
    ))?;

    duration.to_std().context("duration should be positive")
}

#[cfg(debug_assertions)]
const WORKING_DIR: &str = "../../";
#[cfg(not(debug_assertions))]
const WORKING_DIR: &str = "";

// this is because File is not Sync and I don't want to use LazyLock,
// so I'm just gonna make it so that the static can only be used from one thread
thread_local! {
    static LOG_FILE: LazyCell<RefCell<File>> = LazyCell::new(|| {
        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");

        let exe_path =
            env::current_exe().expect("Failed to determine the path of the current executable");
        let log_folder = exe_path
            .parent()
            .unwrap()
            .join(WORKING_DIR)
            .join("logs");
        if !log_folder.exists() {
            fs::create_dir(&log_folder).expect("Couldn't create the log folder");
        }
        let log_file = log_folder
            .join(format!("log_{}", since_the_epoch.as_millis()));

        let file = OpenOptions::new()
            .write(true)
            .append(true) // Or use .truncate(true) if you want to overwrite
            .create(true)
            .open(log_file)
            .expect("open should have worked");

        RefCell::new(file)
    });
}

/// Since the elevated process won't have a stdout,
/// The only way to print debug information is to write it to a file.
fn log_to_file(s: impl Display) -> Result<()> {
    println!("{}", s);
    LOG_FILE.with(|log_file| {
        let mut log_file = log_file.borrow_mut();
        writeln!(log_file, "{}", s).context("failed to log to file")
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Config {
    blocked_sites: Vec<String>,
    browser_exe: String,
    enable_downtime: TimeConfig,
    disable_downtime: TimeConfig,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
struct TimeConfig {
    hour: u32,
    minute: u32,
}

fn read_config() -> Result<Config> {
    let exe_path =
        env::current_exe().expect("Failed to determine the path of the current executable");
    let config_path = exe_path
        .parent()
        .unwrap()
        .join(WORKING_DIR)
        .join("settings.toml");
    let config = read_to_string(config_path)
        .context("there must be a settings.toml file in the same directory as the binary")?;

    toml::from_str(&config).context(
        "Incorrect settings.toml format, go to GitHub for an example of what the settings.toml file should look like",
    )
}

/// returns if downtime is currently enabled or disabled
fn read_state() -> Result<bool> {
    let exe_path =
        env::current_exe().expect("Failed to determine the path of the current executable");
    let save_state_path = exe_path.parent().unwrap().join(WORKING_DIR).join("state");

    if let Ok(save_state) = read_to_string(save_state_path) {
        Ok(save_state.contains("enabled"))
    } else {
        Ok(false)
    }
}

fn save_state(enabled: bool) -> Result<()> {
    let exe_path =
        env::current_exe().expect("Failed to determine the path of the current executable");
    let save_state_path = exe_path.parent().unwrap().join(WORKING_DIR).join("state");
    let mut save_state_file =
        File::create(save_state_path).context("failed to open save state file")?;

    write!(
        save_state_file,
        "{}",
        if enabled { "enabled" } else { "disabled" }
    )?;

    Ok(())
}

const HOSTS_PATH: &str = r"C:\windows\system32\drivers\etc\hosts";

fn write_to_hosts(domains: &[String]) -> Result<()> {
    // create a backup of the hosts file (so it can be restored later)
    let exe_path =
        env::current_exe().expect("Failed to determine the path of the current executable");
    let backup_path = exe_path
        .parent()
        .unwrap()
        .join(WORKING_DIR)
        .join("hosts_backup");
    log_to_file(format!("backing up to {}", backup_path.display()))?;
    let contents =
        read_to_string(HOSTS_PATH).context("failed to read etc/hosts (for backing up)")?;
    write(&backup_path, &contents).context("failed to backup etc/hosts")?;

    // Open the file with write permissions.
    let mut file = OpenOptions::new()
        .write(true)
        .append(true) // Or use .truncate(true) if you want to overwrite
        .open(HOSTS_PATH)
        .context("failed to open etc/hosts")?;

    for domain in domains {
        writeln!(file, "0.0.0.0 {}", domain)?;
    }
    Ok(())
}

fn restore_hosts() -> Result<()> {
    // get the backup
    let exe_path =
        env::current_exe().expect("Failed to determine the path of the current executable");
    let backup_path = exe_path
        .parent()
        .unwrap()
        .join(WORKING_DIR)
        .join("hosts_backup");
    log_to_file(format!("backing up to {}", backup_path.display()))?;
    let contents = read_to_string(backup_path).context("failed to read the backup file")?;

    write(HOSTS_PATH, &contents).context("failed to write the backup to etc/hosts")?;
    Ok(())
}

fn kill_browser(browser_exe: &str) -> Result<Output> {
    println!("killing browser");
    std::process::Command::new("taskkill")
        //.arg("/f")
        .arg("/im")
        .arg(browser_exe)
        .output()
        .context("failed to kill the browser")
}
