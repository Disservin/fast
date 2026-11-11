//! Native file-picker alternatives for Linux sessions without a desktop portal.

use std::path::PathBuf;

/// Opens a native picker on a worker; an empty selection means the user cancelled.
#[cfg(target_os = "linux")]
pub fn pick(
    title: &str,
    save_name: Option<&str>,
    directory: bool,
    multiple: bool,
) -> anyhow::Result<Vec<PathBuf>> {
    if std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .is_ok_and(|release| release.to_ascii_lowercase().contains("microsoft"))
    {
        return pick_windows(title, save_name, directory, multiple);
    }
    pick_linux(title, save_name, directory, multiple)
}

/// Other platforms already use GPUI's native dialog implementation.
#[cfg(not(target_os = "linux"))]
pub fn pick(_: &str, _: Option<&str>, _: bool, _: bool) -> anyhow::Result<Vec<PathBuf>> {
    anyhow::bail!("No alternative native file picker available")
}

/// Builds a Windows common-dialog request with safely quoted labels and filenames.
#[cfg(target_os = "linux")]
fn windows_script(title: &str, save_name: Option<&str>, directory: bool, multiple: bool) -> String {
    let title = title.replace('\'', "''");
    let dialog = if directory {
        format!(
            "$dialog = New-Object System.Windows.Forms.FolderBrowserDialog; $dialog.Description = '{title}';"
        )
    } else if let Some(name) = save_name {
        format!(
            "$dialog = New-Object System.Windows.Forms.SaveFileDialog; $dialog.Title = '{title}'; $dialog.FileName = '{}'; $dialog.OverwritePrompt = $true; $dialog.CheckPathExists = $true;",
            name.replace('\'', "''")
        )
    } else {
        format!(
            "$dialog = New-Object System.Windows.Forms.OpenFileDialog; $dialog.Title = '{title}'; $dialog.Multiselect = ${}; $dialog.CheckFileExists = $true;",
            if multiple { "true" } else { "false" }
        )
    };
    let dialog = if save_name.is_some_and(|name| name.to_ascii_lowercase().ends_with(".pgn")) {
        let filter = format!(
            "{} (*.pgn)|*.pgn|{} (*.*)|*.*",
            ui::i18n::t("PGN files"),
            ui::i18n::t("All files")
        )
        .replace('\'', "''");
        format!(
            "{dialog} $dialog.Filter = '{filter}'; $dialog.DefaultExt = 'pgn'; $dialog.AddExtension = $true;"
        )
    } else {
        dialog
    };
    let selected = if directory {
        "$dialog.SelectedPath"
    } else if save_name.is_some() {
        "$dialog.FileName"
    } else {
        "$dialog.FileNames"
    };
    format!(
        "$ErrorActionPreference = 'Stop'; [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding; Add-Type -AssemblyName System.Windows.Forms; {dialog} try {{ if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ ConvertTo-Json -Compress -InputObject @({selected}) }} else {{ '[]' }} }} finally {{ $dialog.Dispose() }}"
    )
}

/// Reads a native Windows selection and translates each path back into the WSL filesystem.
#[cfg(target_os = "linux")]
fn windows_paths(
    output: &[u8],
    mut translate: impl FnMut(&str) -> anyhow::Result<PathBuf>,
) -> anyhow::Result<Vec<PathBuf>> {
    let text = std::str::from_utf8(output)?
        .trim_start_matches('\u{feff}')
        .trim();
    let paths: Vec<String> = serde_json::from_str(text)?;
    paths.iter().map(|path| translate(path)).collect()
}

/// Shows Windows' standard file or folder dialog through WSL interoperability.
#[cfg(target_os = "linux")]
fn pick_windows(
    title: &str,
    save_name: Option<&str>,
    directory: bool,
    multiple: bool,
) -> anyhow::Result<Vec<PathBuf>> {
    use std::process::Command;
    let script = windows_script(title, save_name, directory, multiple);
    let run = |executable| {
        Command::new(executable)
            .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
            .output()
    };
    let output = run("powershell.exe").or_else(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            run("/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe")
        } else {
            Err(err)
        }
    })?;
    anyhow::ensure!(
        output.status.success(),
        "Windows file picker failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    windows_paths(&output.stdout, |path| {
        let output = Command::new("wslpath").args(["-u", path]).output()?;
        anyhow::ensure!(
            output.status.success(),
            "Could not translate the selected Windows path"
        );
        let path = String::from_utf8(output.stdout)?;
        Ok(PathBuf::from(path.trim_end_matches(['\r', '\n'])))
    })
}

/// Tries the desktop's native chooser when its portal service is unavailable.
#[cfg(target_os = "linux")]
fn pick_linux(
    title: &str,
    save_name: Option<&str>,
    directory: bool,
    multiple: bool,
) -> anyhow::Result<Vec<PathBuf>> {
    use std::process::Command;
    let mut zenity = Command::new("zenity");
    zenity.args(["--file-selection", "--title", title]);
    if directory {
        zenity.arg("--directory");
    }
    if let Some(name) = save_name {
        zenity.args(["--save", "--confirm-overwrite", "--filename", name]);
    }
    if multiple {
        zenity.args(["--multiple", "--separator=\n"]);
    }
    let mut kde = Command::new("kdialog");
    kde.args(["--title", title]);
    if directory {
        kde.args(["--getexistingdirectory", "."]);
    } else if let Some(name) = save_name {
        kde.args(["--getsavefilename", name]);
    } else {
        kde.args(["--getopenfilename", "."]);
        if multiple {
            kde.args(["--multiple", "--separate-output"]);
        }
    }
    for mut command in [zenity, kde] {
        if let Ok(output) = command.output() {
            if output.status.success() {
                return Ok(String::from_utf8(output.stdout)?
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(PathBuf::from)
                    .collect());
            }
            if output.status.code() == Some(1) {
                return Ok(Vec::new());
            }
        }
    }
    anyhow::bail!("No native file picker available")
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Cancellation performs no path conversion, while Unicode and spaces survive selection.
    #[test]
    fn reads_windows_selections_and_cancel_without_opening_a_dialog() {
        assert!(
            windows_paths(b"[]", |_| panic!("Cancelled selections have no paths"))
                .unwrap()
                .is_empty()
        );
        let mut seen = Vec::new();
        let selected = windows_paths(
            "[\"C:\\\\Chess games\\\\Müller.pgn\",\"D:\\\\Other.pgn\"]".as_bytes(),
            |path| {
                seen.push(path.to_owned());
                Ok(PathBuf::from(format!("/translated/{path}")))
            },
        )
        .unwrap();
        assert_eq!(seen, ["C:\\Chess games\\Müller.pgn", "D:\\Other.pgn"]);
        assert_eq!(selected.len(), 2);
        assert!(windows_paths(b"not JSON", |_| unreachable!()).is_err());
        assert!(
            windows_paths(b"[\"C:\\\\bad.pgn\"]", |_| anyhow::bail!(
                "translation failed"
            ))
            .is_err()
        );
    }
}
