//! Bounded, atomic command-line adapter for `svg2excal-core`.

// A one-shot CLI intentionally uses synchronous local file I/O; it has no async runtime.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use svg2excal_core::{ConversionOptions, ConversionProfile, convert};
use tempfile::NamedTempFile;

const STDIO_INPUT_LIMIT: u64 = 16 * 1024 * 1024;

#[derive(Debug, Parser)]
#[command(name = "svg2excal", version, about)]
struct Arguments {
    /// Input SVG/SVGZ path, or `-` for standard input.
    input: PathBuf,
    /// Output .excalidraw path, or `-` for standard output.
    #[arg(short, long, default_value = "-")]
    output: PathBuf,
    /// Conversion profile.
    #[arg(long, value_enum, default_value_t = Profile::Balanced)]
    profile: Profile,
    /// Optional JSON report path. The report is written atomically.
    #[arg(long)]
    report: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Profile {
    Balanced,
    Editable,
    Fidelity,
    Strict,
}

impl From<Profile> for ConversionProfile {
    fn from(value: Profile) -> Self {
        match value {
            Profile::Balanced => Self::Balanced,
            Profile::Editable => Self::Editable,
            Profile::Fidelity => Self::Fidelity,
            Profile::Strict => Self::Strict,
        }
    }
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    run(&arguments)
}

fn run(arguments: &Arguments) -> Result<()> {
    if arguments
        .report
        .as_deref()
        .is_some_and(|path| path.as_os_str() == "-")
    {
        bail!("the report cannot share standard output with the document");
    }
    let paths = resolved_paths(arguments)?;
    let input = read_bounded(paths.input.as_deref(), STDIO_INPUT_LIMIT)?;
    let options = ConversionOptions::builder()
        .profile(arguments.profile.into())
        .build();
    let result = convert(&input, &options).context("conversion failed")?;
    let document = result
        .document
        .to_pretty_json_with_limits(&options.limits)
        .context("target serialization failed")?;
    let report = arguments
        .report
        .as_ref()
        .map(|_| serde_json::to_vec_pretty(&result.report).context("report serialization"))
        .transpose()?;
    write_output(paths.output.as_deref(), document.as_bytes())?;
    if let (Some(report_path), Some(report)) = (paths.report.as_deref(), report) {
        write_atomic(report_path, &report)?;
    }
    Ok(())
}

#[derive(Debug)]
struct ResolvedPaths {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    report: Option<PathBuf>,
}

fn resolved_paths(arguments: &Arguments) -> Result<ResolvedPaths> {
    let input = resolved_path(&arguments.input, true)?;
    let output = resolved_path(&arguments.output, false)?;
    let report = arguments
        .report
        .as_deref()
        .map(|path| resolved_path(path, false))
        .transpose()?
        .flatten();
    if output
        .as_ref()
        .is_some_and(|path| input.as_ref() == Some(path))
        || report
            .as_ref()
            .is_some_and(|path| input.as_ref() == Some(path))
    {
        bail!("input, document, and report paths must not identify the same file");
    }
    if output
        .as_ref()
        .is_some_and(|path| report.as_ref() == Some(path))
    {
        bail!("the document and report paths must differ");
    }
    Ok(ResolvedPaths {
        input,
        output,
        report,
    })
}

fn resolved_path(path: &Path, must_exist: bool) -> Result<Option<PathBuf>> {
    if path.as_os_str() == "-" {
        return Ok(None);
    }
    if must_exist || fs::symlink_metadata(path).is_ok() {
        return fs::canonicalize(path)
            .map(Some)
            .context("path is unavailable");
    }
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .context("path needs a file name")?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    Ok(Some(
        fs::canonicalize(parent)
            .context("path directory is unavailable")?
            .join(file_name),
    ))
}

fn read_bounded(path: Option<&Path>, limit: u64) -> Result<Vec<u8>> {
    let mut reader: Box<dyn Read> = if let Some(path) = path {
        let metadata = fs::metadata(path).context("input metadata is unavailable")?;
        if !metadata.is_file() {
            bail!("input must be a regular file");
        }
        Box::new(File::open(path).context("input could not be opened")?)
    } else {
        Box::new(io::stdin().lock())
    };
    let capacity = usize::try_from(limit.min(64 * 1024)).context("input limit overflow")?;
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .context("input could not be read")?;
    if u64::try_from(bytes.len()).context("input length overflow")? > limit {
        bail!("input exceeds the 16 MiB adapter limit");
    }
    Ok(bytes)
}

fn write_output(path: Option<&Path>, bytes: &[u8]) -> Result<()> {
    if let Some(path) = path {
        write_atomic(path, bytes)
    } else {
        let mut stdout = io::stdout().lock();
        stdout.write_all(bytes).context("standard output failed")?;
        stdout.flush().context("standard output failed")
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let file_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .context("output path needs a file name")?;
    if file_name == "." || file_name == ".." {
        bail!("output file name is invalid");
    }
    let parent = path
        .parent()
        .context("resolved output path needs a parent")?;
    let mut temporary =
        NamedTempFile::new_in(parent).context("temporary output could not be created")?;
    temporary
        .write_all(bytes)
        .context("temporary output could not be written")?;
    temporary
        .as_file()
        .sync_all()
        .context("temporary output could not be synchronized")?;
    let destination = parent.join(file_name);
    temporary
        .persist(&destination)
        .map_err(|error| error.error)
        .with_context(|| format!("output could not be committed: {}", destination.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_write_complete_document_atomically() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("result.excalidraw");
        write_atomic(&output, b"complete")?;
        assert_eq!(fs::read(output)?, b"complete");
        Ok(())
    }

    #[test]
    fn test_should_reject_lexically_aliased_destinations() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let input = directory.path().join("input.svg");
        fs::write(&input, b"<svg/>")?;
        let arguments = Arguments {
            input,
            output: directory.path().join("result.excalidraw"),
            profile: Profile::Balanced,
            report: Some(directory.path().join(".").join("result.excalidraw")),
        };
        assert!(resolved_paths(&arguments).is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_should_reject_symlink_alias_to_input() -> Result<()> {
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir()?;
        let input = directory.path().join("input.svg");
        let alias = directory.path().join("output.excalidraw");
        fs::write(&input, b"<svg/>")?;
        symlink(&input, &alias)?;
        let arguments = Arguments {
            input,
            output: alias,
            profile: Profile::Balanced,
            report: None,
        };
        assert!(resolved_paths(&arguments).is_err());
        Ok(())
    }
}
