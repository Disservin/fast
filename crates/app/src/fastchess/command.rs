//! Pasted command lines and configs, and reading flags back out of argument lists.

use super::*;

/// What the user pasted to start from.
#[derive(Clone, Debug, PartialEq)]
pub enum Pasted {
    /// A `config.json` fastchess saved, to continue.
    Config(serde_json::Value),
    /// A fastchess command line.
    Args(Vec<String>),
}

pub fn parse_paste(text: &str) -> Result<Pasted> {
    let text = text.trim();
    ensure!(
        !text.is_empty(),
        "Paste a fastchess config.json or command line"
    );
    if text.starts_with('{') {
        let value: serde_json::Value =
            serde_json::from_str(text).context("The pasted config is not valid JSON")?;
        let engines = value
            .get("engines")
            .and_then(|engines| engines.as_array())
            .context("The pasted config has no \"engines\" list; is it a fastchess config.json?")?;
        ensure!(
            engines.len() >= 2,
            "The pasted config has fewer than two engines"
        );
        return Ok(Pasted::Config(value));
    }
    let mut args = split_args(text)?;
    // A copied command usually starts with the program itself.
    if args.first().is_some_and(|first| {
        !first.starts_with('-')
            && Path::new(first)
                .file_stem()
                .is_some_and(|stem| stem.to_string_lossy().eq_ignore_ascii_case("fastchess"))
    }) {
        args.remove(0);
    }
    ensure!(
        args.iter()
            .any(|arg| arg == "-engine" || arg == "-config" || arg == "-quick"),
        "The pasted command has no -engine, -config or -quick option"
    );
    Ok(Pasted::Args(args))
}

impl Pasted {
    /// Arguments to run, with the config saved into `dir` unless the paste names its own,
    /// and where that config will be.
    pub fn launch(&self, dir: &Path, cwd: &Path) -> Result<(Vec<String>, PathBuf)> {
        match self {
            Self::Config(value) => {
                let path = dir.join("config.json");
                std::fs::write(&path, serde_json::to_vec_pretty(value)?)
                    .context("Saving the pasted config")?;
                Ok((resume_args(&path), path))
            }
            Self::Args(args) => {
                let mut args = args.clone();
                let named = config_outname(&args).map(|name| cwd.join(name));
                let config = match named {
                    Some(path) => path,
                    None => {
                        let path = dir.join("config.json");
                        args.extend(["-config".into(), format!("outname={}", path.display())]);
                        path
                    }
                };
                Ok((args, config))
            }
        }
    }
}

/// The `outname=` of a `-config` option, if the command sets one.
pub(super) fn config_outname(args: &[String]) -> Option<String> {
    let mut in_config = false;
    let mut found = None;
    for arg in args {
        if arg.starts_with('-') {
            in_config = arg == "-config";
        } else if in_config && let Some(name) = arg.strip_prefix("outname=") {
            found = Some(name.to_owned());
        }
    }
    found
}

/// Splits a command line the way a shell would, for the forms people paste: quotes,
/// backslash and `^` line continuations. Backslashes before other characters stay, so
/// Windows paths survive.
pub fn split_args(text: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = text.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') if matches!(chars.peek(), Some('"' | '\\')) => {
                current.push(chars.next().unwrap_or('\\'));
            }
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                started = true;
            }
            (None, '\\' | '^') if matches!(chars.peek(), Some('\n' | '\r')) => {
                while matches!(chars.peek(), Some('\n' | '\r')) {
                    chars.next();
                }
            }
            (None, '\\') if matches!(chars.peek(), Some(' ' | '"' | '\'' | '\\')) => {
                current.push(chars.next().unwrap_or('\\'));
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    ensure!(quote.is_none(), "Unclosed quote in the command line");
    if started {
        args.push(current);
    }
    Ok(args)
}

/// Quotes arguments for display, so the shown command can be pasted into a shell.
pub fn display_command(program: &str, args: &[String]) -> String {
    let quote = |arg: &str| {
        if !arg.is_empty()
            && arg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_=./+:,@%".contains(c))
        {
            arg.to_owned()
        } else {
            format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
        }
    };
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Engines in the order a command gives them, as (name, executable).
pub(crate) fn engines(args: &[String]) -> Vec<(String, String)> {
    args.iter()
        .enumerate()
        .filter(|(_, arg)| arg.as_str() == "-engine")
        .map(|(index, _)| {
            let values: Vec<&str> = args[index + 1..]
                .iter()
                .take_while(|arg| !arg.starts_with('-'))
                .map(String::as_str)
                .collect();
            (
                kv(&values, "name").unwrap_or("engine").to_owned(),
                kv(&values, "cmd").unwrap_or("").to_owned(),
            )
        })
        .collect()
}

/// The values after a flag, up to the next flag.
pub(crate) fn flag_values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    let Some(start) = args.iter().position(|arg| arg.as_str() == flag) else {
        return Vec::new();
    };
    args[start + 1..]
        .iter()
        .take_while(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect()
}

pub(crate) fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    flag_values(args, flag).first().copied()
}

/// The value of `key=value` among `values`.
pub(crate) fn kv<'a>(values: &[&'a str], key: &str) -> Option<&'a str> {
    values
        .iter()
        .find_map(|&value| value.strip_prefix(key)?.strip_prefix('='))
}

pub(crate) fn has(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg.as_str() == flag)
}
