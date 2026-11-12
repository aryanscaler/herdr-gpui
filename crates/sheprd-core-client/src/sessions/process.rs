use super::Error;
use crate::Result;
#[cfg(not(target_os = "macos"))]
use sheprd_client::ScriptLimits;
#[cfg(not(any(windows, target_os = "macos")))]
use sheprd_client::{ScriptHost, run_script};
#[cfg(not(target_os = "macos"))]
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
pub(super) fn read(pid: u32) -> Result<Process> {
    if pid == 0 {
        return Err(Error::Identity.into());
    }
    let root = std::env::var_os("SystemRoot").ok_or(Error::Identity)?;
    let program =
        std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut command = sheprd_client::new_command(program);
    command.args(["-NoProfile", "-NonInteractive", "-Command", &format!(
        "$ErrorActionPreference='Stop'; $p=Get-CimInstance Win32_Process -Filter 'ProcessId={pid}'; if(!$p){{exit 1}}; [Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); Write-Output $p.ParentProcessId; Write-Output ('windows:'+ $p.CreationDate.ToUniversalTime().Ticks); Write-Output $p.Name"
    )]);
    let mut output = Vec::new();
    sheprd_client::run_script_command(
        command,
        &[][..],
        &mut output,
        ScriptLimits {
            output: 1024,
            idle: Duration::from_secs(5),
        },
        &AtomicBool::new(false),
    )
    .map_err(Error::Probe)?;
    parse(&output)
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
