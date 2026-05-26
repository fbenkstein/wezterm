//! WezTerm installer: signs, notarizes, and installs the sibling WezTerm.app
//! to ~/Applications using a Developer ID Application identity from the user's
//! login keychain.
//!
//! Notarization expects a stored keychain profile named `wezterm-installer`;
//! see README.md.

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{anyhow, bail, Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal;

const NOTARYTOOL_PROFILE: &str = "wezterm-installer";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\nerror: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let installer_dir = env::current_exe()?
        .parent()
        .ok_or_else(|| anyhow!("installer has no parent directory"))?
        .to_path_buf();

    let app_path = installer_dir.join("WezTerm.app");
    if !app_path.is_dir() {
        bail!(
            "WezTerm.app not found next to installer (looked for {})",
            app_path.display()
        );
    }

    let identity = find_developer_id()
        .context("looking up Developer ID Application identity in your keychain")?;

    let home = env::var_os("HOME").context("HOME is not set")?;
    let dest_dir = PathBuf::from(home).join("Applications");
    let dest_app = dest_dir.join("WezTerm.app");

    println!("WezTerm Installer");
    println!();
    println!("  App:      {}", app_path.display());
    println!("  Identity: {identity}");
    println!("  Install:  {}", dest_app.display());
    println!();
    println!("Will sign + notarize the app on this machine, then install to ~/Applications.");
    println!("Notarization uses the `{NOTARYTOOL_PROFILE}` keychain profile (xcrun notarytool).");
    println!();

    if !prompt_yes("Proceed?")? {
        println!("Aborted.");
        return Ok(());
    }

    step("Stripping quarantine");
    run_cmd(Command::new("xattr").arg("-cr").arg(&app_path))?;

    step("Signing");
    run_cmd(
        Command::new("codesign")
            .args([
                "--force",
                "--deep",
                "--options",
                "runtime",
                "--timestamp",
                "--sign",
            ])
            .arg(&identity)
            .arg(&app_path),
    )?;

    step("Notarizing (this may take several minutes)");
    let tmp_zip = std::env::temp_dir().join("WezTerm-notarize.zip");
    let _ = std::fs::remove_file(&tmp_zip);
    run_cmd(
        Command::new("ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(&app_path)
            .arg(&tmp_zip),
    )?;
    run_cmd(
        Command::new("xcrun")
            .args(["notarytool", "submit"])
            .arg(&tmp_zip)
            .args(["--keychain-profile", NOTARYTOOL_PROFILE, "--wait"]),
    )?;
    let _ = std::fs::remove_file(&tmp_zip);

    step("Stapling notarization ticket");
    run_cmd(
        Command::new("xcrun")
            .args(["stapler", "staple"])
            .arg(&app_path),
    )?;

    println!();
    println!("Signed and notarized. Ready to install.");
    if !prompt_yes("Install?")? {
        println!(
            "Aborted. Signed/notarized .app left at {}.",
            app_path.display()
        );
        return Ok(());
    }

    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("creating {}", dest_dir.display()))?;
    if dest_app.exists() {
        std::fs::remove_dir_all(&dest_app)
            .with_context(|| format!("removing existing {}", dest_app.display()))?;
    }
    run_cmd(Command::new("ditto").arg(&app_path).arg(&dest_app))?;

    println!();
    println!("Installed to {}.", dest_app.display());
    Ok(())
}

fn step(msg: &str) {
    println!("→ {msg}...");
}

fn run_cmd(cmd: &mut Command) -> Result<()> {
    let prog = cmd.get_program().to_string_lossy().into_owned();
    let status = cmd.status().with_context(|| format!("running {prog}"))?;
    if !status.success() {
        bail!("{prog} exited with {status}");
    }
    Ok(())
}

fn find_developer_id() -> Result<String> {
    let output = Command::new("security")
        .args(["find-identity", "-v", "-p", "codesigning"])
        .output()
        .context("running `security find-identity`")?;
    if !output.status.success() {
        bail!(
            "`security find-identity` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&output.stdout);

    // Lines look like:  1) ABC123ABCD "Developer ID Application: Frank (TEAM)"
    let ids: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let start = line.find('"')? + 1;
            let end = line.rfind('"')?;
            if end <= start {
                return None;
            }
            let cn = &line[start..end];
            cn.starts_with("Developer ID Application:")
                .then(|| cn.to_string())
        })
        .collect();

    match ids.as_slice() {
        [] => bail!(
            "no `Developer ID Application` identity found in your login keychain.\n\
             Run `security find-identity -v -p codesigning` to list what's available."
        ),
        [single] => Ok(single.clone()),
        many => bail!(
            "multiple Developer ID Application identities found ({}):\n  {}\n\
             This installer does not yet support choosing among them.",
            many.len(),
            many.join("\n  ")
        ),
    }
}

fn prompt_yes(msg: &str) -> Result<bool> {
    print!("{msg} [Y/n] ");
    io::stdout().flush()?;

    terminal::enable_raw_mode()?;
    let answer = loop {
        match event::read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => break Ok(true),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => break Ok(false),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Ok(false);
                }
                _ => continue,
            },
            Ok(_) => continue,
            Err(e) => break Err(e.into()),
        }
    };
    terminal::disable_raw_mode()?;

    if let Ok(yes) = &answer {
        println!("{}", if *yes { 'y' } else { 'n' });
    }
    answer
}
