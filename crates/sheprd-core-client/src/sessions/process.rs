use super::Error;
use crate::Result;
#[cfg(not(any(windows, target_os = "macos")))]
use sheprd_client::ScriptLimits;
#[cfg(not(any(windows, target_os = "macos")))]
use sheprd_client::{ScriptHost, run_script};
#[cfg(not(any(windows, target_os = "macos")))]
use std::{sync::atomic::AtomicBool, time::Duration};

pub(super) struct Process {
    pub(super) parent: u32,
    pub(super) started: String,
    name: String,
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(super) fn read(pid: u32) -> Result<Process> {
    if pid == 0 {
        return Err(Error::Identity.into());
    }
    // Linux supplies exact start ticks, even when comm contains parentheses.
    let script = format!(
        r#"set -f
case "$(uname -s)" in
Linux)
 IFS= read -r stat < /proc/{pid}/stat || exit 1
 fields=${{stat##*) }}
 set -- $fields
 [ "$#" -ge 20 ] || exit 1
 comm=${{stat#*(}}
 comm=${{comm%)*}}
 printf '%s\nlinux:%s\n%s\n' "$2" "${{20}}" "$comm"
 ;;
*) exit 1;;
esac
"#
    );
    let mut output = Vec::new();
    run_script(
        ScriptHost::Local,
        &script,
        &[][..],
        &mut output,
        ScriptLimits {
            output: 1024,
            idle: Duration::from_secs(2),
        },
        &AtomicBool::new(false),
    )
    .map_err(Error::Probe)?;
    parse(&output)
}

#[cfg(target_os = "macos")]
pub(super) fn read(pid: u32) -> Result<Process> {
    use libproc::libproc::{bsd_info::BSDInfo, proc_pid::pidinfo};
    let pid = i32::try_from(pid)
        .ok()
        .filter(|pid| *pid > 0)
        .ok_or(Error::Identity)?;
    let info = pidinfo::<BSDInfo>(pid, 0).map_err(|_| Error::Identity)?;
    let name = info
        .pbi_name
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(|byte| byte as u8)
        .collect::<Vec<_>>();
    Ok(Process {
        parent: info.pbi_ppid,
        started: format!("darwin:{}:{}", info.pbi_start_tvsec, info.pbi_start_tvusec),
        name: String::from_utf8(name).map_err(|_| Error::Identity)?,
    })
}

#[cfg(windows)]
mod powershell;

#[cfg(windows)]
pub(super) fn read(pid: u32) -> Result<Process> {
    if pid == 0 {
        return Err(Error::Identity.into());
    }
    let root = std::env::var_os("SystemRoot").ok_or(Error::Identity)?;
    let program =
        std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = sheprd_client::new_command(program);
    command.args(["-NoProfile", "-NonInteractive", "-Command", &format!(
        "$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; $p=Get-CimInstance Win32_Process -Filter 'ProcessId={pid}'; if(!$p){{exit {ABSENT}}}; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); Write-Output $p.ParentProcessId; Write-Output ('windows:'+ $p.CreationDate.ToUniversalTime().Ticks); Write-Output $p.Name"
    )]);
    let output = powershell::run(command).map_err(Error::Probe)?;
    parse(&output)
}

/// Exit code of the Windows probe when no process has the PID, so absence is
/// never confused with a failed probe.
#[cfg(windows)]
const ABSENT: i32 = 3;

/// `None` only when no process has `pid`; a failed probe stays an error, so
/// callers deciding whether an old process is gone never treat doubt as gone.
pub(super) fn birth(pid: u32) -> Result<Option<String>> {
    #[cfg(unix)]
    let gone = || {
        i32::try_from(pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
            .is_some_and(|pid| {
                rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH)
            })
    };
    #[cfg(unix)]
    if gone() {
        return Ok(None);
    }
    match read(pid) {
        Ok(process) => Ok(Some(process.started)),
        // It exited between the check and the read.
        #[cfg(unix)]
        Err(_) if gone() => Ok(None),
        #[cfg(windows)]
        Err(crate::Error::Sessions(Error::Probe(sheprd_client::Error::ScriptExit {
            status,
            ..
        }))) if status.code() == Some(ABSENT) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(not(target_os = "macos"))]
fn parse(output: &[u8]) -> Result<Process> {
    let text = std::str::from_utf8(output).map_err(|_| Error::Identity)?;
    let mut lines = text.lines();
    let parent = lines
        .next()
        .and_then(|word| word.trim().parse().ok())
        .ok_or(Error::Identity)?;
    let started = lines
        .next()
        .ok_or(Error::Identity)?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let name = lines.next().ok_or(Error::Identity)?.trim().to_owned();
    if started.is_empty() || name.is_empty() || lines.next().is_some() {
        return Err(Error::Identity.into());
    }
    Ok(Process {
        parent,
        started,
        name,
    })
}

pub(super) fn descends(mut pid: u32, ancestor: u32) -> Result<bool> {
    for depth in 0..32 {
        if pid == ancestor {
            return Ok(true);
        }
        let process = read(pid)?;
        // Only shell hook launchers may intervene. A nested agent inherits
        // HC_TERMINAL_ID too; ancestry alone would misattribute its session.
        if depth > 0
            && !matches!(
                process
                    .name
                    .rsplit('/')
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('-'),
                "sh" | "bash"
                    | "zsh"
                    | "dash"
                    | "fish"
                    | "ksh"
                    | "cmd.exe"
                    | "powershell.exe"
                    | "pwsh.exe"
            )
        {
            return Ok(false);
        }
        let parent = process.parent;
        if parent == 0 || parent == pid {
            return Ok(false);
        }
        pid = parent;
    }
    Ok(false)
}
