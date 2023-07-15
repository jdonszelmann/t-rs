use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use clap::{Parser, Subcommand};
use color_eyre::eyre::{Context, ContextCompat};
use color_eyre::Result;
use fs_extra::dir::CopyOptions;

const TEMPDIR_PREFIX: &str = "T-RS-TEMPDIR";
const TEMPDIRS: &str = "tempdirs";

/// # Usage:
///
/// Put the following in your bashrc or zshrc file.
///
/// `function t() { cd $(t-rs $@ | tail -n 1) }`
///
/// Then use the `t` command.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Cli {
    /// Optional name to operate on
    name: Option<String>,

    /// The location to symlink the temporary directories to for easy access.
    ///
    /// This allows you to easily see the tempdirs in your file browser for example.
    /// By default this is `$HOME/tempdirs`.
    #[clap(long, env)]
    tempdirs: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Subcommand, Debug)]
enum CliCommand {
    /// persist the current tempdir
    Persist {
        name: Option<String>
    },

    /// don't show up in the list of tempdirs
    Hidden,

    Shell,

    /// rename the current or specified tempdir
    Rename {
        from: Option<String>,
        to: Option<String>,
    },

    /// delete all tempdirs
    #[clap(alias = "d")]
    Delete {
        // delete all directories
        #[arg(long, short)]
        all: bool,

        name: Option<String>,
    },

    #[clap(alias = "s")]
    Status,
}


fn main() -> Result<()> {
    color_eyre::install()?;

    let args: Cli = Cli::parse();

    let home = home::home_dir()
        .wrap_err("couldn't get home directory")?;
    let tempdirs = args.tempdirs.unwrap_or_else(|| home.join(TEMPDIRS));
    if !tempdirs.exists() {
        std::fs::create_dir_all(&tempdirs)
            .wrap_err(format!("create tempdirs ({tempdirs:?})"))?;
    }

    let name = args.name
        .clone()
        .map(Ok)
        .unwrap_or_else(|| new_name(&tempdirs))?;

    let cwd = std::env::current_dir().wrap_err("get current dir")?;
    let pwd = {
        let pwd = std::env::var("PWD").wrap_err("get pwd")?;
        if pwd.is_empty() {
            None
        } else {
            Some(PathBuf::from(pwd))
        }
    };

    let orig = pwd.clone().unwrap_or(cwd.clone());

    let go_to: Option<PathBuf> = match args.command {
        None => {
            Some(create_tempdir(&tempdirs, &name, &cwd, pwd.as_deref(), true)?)
        }
        Some(CliCommand::Shell) => {
            shell(&tempdirs, &name, &cwd, &pwd)?;
            None
        }
        Some(CliCommand::Persist { name }) => {
            fn persist(p: &Path) -> Result<()> {
                if !p.is_symlink() {
                    eprintln!("{p:?} was already persistent");

                    return Ok(());
                }

                let original_target = std::fs::read_link(p).wrap_err("read link")?;

                // unlink the original reference
                symlink::remove_symlink_auto(&p).wrap_err("unlink")?;

                eprintln!("moving from {original_target:?} to {p:?}");
                // but then move the original temporary dir to where the symlink used to be
                fs_extra::dir::move_dir(&original_target, p, &CopyOptions {
                    copy_inside: true,
                    ..Default::default()
                }).wrap_err("copy to original symlink location")?;

                eprintln!("{:?} is now persistent", p);
                Ok(())
            }

            if let Some(i) = in_tempdir(&tempdirs, &cwd, pwd.as_deref()).wrap_err("in tempdir while renaming")? {
                let original_symlink = i.as_path();
                persist(original_symlink)?;

                Some(tempdirs)
            } else if let Some(ref n) = args.name {
                let original_symlink = tempdirs.join(n);
                if !original_symlink.exists() {
                    eprintln!("{original_symlink:?} doesn't exist");
                    None
                } else {
                    persist(&original_symlink)?;

                    Some(tempdirs)
                }
            } else if let Some(ref n) = name {
                let original_symlink = tempdirs.join(n);
                if !original_symlink.exists() {
                    eprintln!("{original_symlink:?} doesn't exist");
                    None
                } else {
                    persist(&original_symlink)?;

                    Some(tempdirs)
                }
            } else {
                eprintln!("not in a tempdir and no tempdir specified (use --all if you want to delete them all)");
                None
            }
        }
        Some(CliCommand::Delete { all: true, name: _ }) => {
            Some(delete_all(&tempdirs)?)
        }
        Some(CliCommand::Delete { all: false, name }) => {
            if let Some(i) = in_tempdir(&tempdirs, &cwd, pwd.as_deref()).wrap_err("in tempdir while renaming")? {
                let original_symlink = i.as_path();
                delete(original_symlink)?;

                Some(tempdirs)
            } else if let Some(ref n) = args.name {
                let original_symlink = tempdirs.join(n);
                if !original_symlink.exists() {
                    eprintln!("{original_symlink:?} doesn't exist");
                    None
                } else {
                    symlink::remove_symlink_auto(&original_symlink).wrap_err(format!("remove symlink {:?}", original_symlink))?;
                    delete(&original_symlink)?;

                    Some(tempdirs)
                }
            } else if let Some(ref n) = name {
                let original_symlink = tempdirs.join(n);
                if !original_symlink.exists() {
                    eprintln!("{original_symlink:?} doesn't exist");
                    None
                } else {
                    delete(&original_symlink)?;

                    Some(tempdirs)
                }
            } else {
                eprintln!("not in a tempdir and no tempdir specified (use --all if you want to delete them all)");
                None
            }
        }
        Some(CliCommand::Hidden) => {
            Some(create_tempdir(&tempdirs, &name, &cwd, pwd.as_deref(), false)?)
        }
        Some(CliCommand::Status) => {
            if let Some(i) = in_tempdir(&tempdirs, &cwd, pwd.as_deref()).wrap_err("in tempdir while getting status")? {
                if i.is_symlink() {
                    eprintln!("currently in tempdir {i:?}");
                    eprintln!("which is a symlink to {:?}", std::fs::read_link(&i).wrap_err("read link")?)
                } else {
                    eprintln!("currently in persisted tempdir {i:?}");
                }
            } else {
                eprintln!("currently not in a tempdir");
            }

            active_tempdirs(&tempdirs)?;
            None
        }
        Some(CliCommand::Rename { from, to }) => {
            if let Some(mut new_name) = from.clone() {
                if let Some(to) = to.clone() {
                    new_name = to;
                }

                if let Some(i) = in_tempdir(&tempdirs, &cwd, pwd.as_deref()).wrap_err("in tempdir while renaming")? {
                    let original_symlink = i.as_path();
                    let new_symlink = tempdirs.join(new_name);

                    if rename(original_symlink, &new_symlink)? {
                        Some(new_symlink)
                    } else {
                        None
                    }
                } else if let Some(ref n) = args.name {
                    let original_symlink = tempdirs.join(n);
                    if !original_symlink.exists() {
                        eprintln!("{original_symlink:?} doesn't exist");
                    } else {
                        let new_symlink = tempdirs.join(new_name);

                        rename(&original_symlink, &new_symlink)?;
                    }
                    None
                } else if let Some(ref n) = from {
                    if to.is_some() {
                        let original_symlink = tempdirs.join(n);
                        if !original_symlink.exists() {
                            eprintln!("{original_symlink:?} doesn't exist");
                        } else {
                            let new_symlink = tempdirs.join(new_name);

                            rename(&original_symlink, &new_symlink)?;
                        }
                        None
                    } else {
                        eprintln!("not in a tempdir and no tempdir specified");
                        None
                    }
                } else {
                    eprintln!("not in a tempdir and no tempdir specified");
                    None
                }
            } else {
                eprintln!("you have to specify a new name");
                None
            }
        }
    };

    // the path printed here is where we will cd to after
    if let Some(i) = go_to {
        println!("\n\n{}", i.to_string_lossy());
    } else {
        println!("\n\n{}", orig.to_string_lossy());
    }
    exit(0)
}

fn shell(tempdirs: &PathBuf, name: &String, cwd: &PathBuf, pwd: &Option<PathBuf>) -> Result<()> {
    let res = create_tempdir(&tempdirs, &name, &cwd, pwd.as_deref(), true)?;
    let mut shell = std::env::var("SHELL").wrap_err("shell envvar")?;
    if shell.is_empty() && Path::new("/bin/zsh").exists() {
        shell = "/bin/zsh".to_string();
    }

    if shell.is_empty() && Path::new("/bin/bash").exists() {
        shell = "/bin/bash".to_string();
    }

    let mut cmd = Command::new(shell);
    // this only sets the cd path which resolves symlinks
    cmd.current_dir(&res);
    // but most shells actually show what path you're in based on `pwd` and PWD
    // so we also set that
    cmd.env("PWD", &res);
    let mut child = cmd.spawn().wrap_err("spawn shell")?;
    child.wait().wrap_err("wait for child")?;

    if res.is_symlink() {
        // find the symlink target
        let target = std::fs::read_link(&res).wrap_err("read link")?;
        // unlink the link so only the /tmp/... remains
        symlink::remove_symlink_auto(&res).wrap_err("unlink")?;
        // remove the /tmp/... dir too
        std::fs::remove_dir_all(&target).wrap_err("remove dir")?;
    }

    Ok(())
}

pub fn rename(old: &Path, new: &Path) -> Result<bool> {
    if new.exists() {
        eprintln!("can't rename to {new:?} because it already exists");
        return Ok(false);
    }

    if !old.is_symlink() {
        // if it's a folder, rename normally
        eprintln!("renaming persistent tempdir {old:?} to {new:?}");
        std::fs::rename(old, new).wrap_err("rename")?;
    } else {
        eprintln!("renaming tempdir {old:?} to {new:?}");
        // else unlink and create a new link
        let target = std::fs::read_link(old).wrap_err("read link")?;
        symlink::remove_symlink_auto(old).wrap_err("unlink old")?;
        symlink::symlink_auto(target, new).wrap_err("symlink new")?;
    }
    Ok(true)
}

pub fn delete(path: &Path) -> Result<()> {
    if path.is_symlink() {
        eprintln!("deleting {:?}", path);
        symlink::remove_symlink_auto(path).wrap_err(format!("remove symlink {:?}", path))?;
    } else {
        eprintln!("deleting {:?} (persistent)", path);
        std::fs::remove_dir_all(path)?;
    }

    Ok(())
}

pub fn active_tempdirs(tempdirs: &Path) -> Result<()> {
    let mut first = true;
    for i in std::fs::read_dir(tempdirs).wrap_err(format!("read {tempdirs:?}"))? {
        let i = i.wrap_err("read direntry")?;
        if first {
            eprintln!("active tempdirs:");
            first = false;
        }

        if i.path().is_symlink() {
            eprintln!("{}", i.path().to_string_lossy());
        } else {
            eprintln!("{} (persistent)", i.path().to_string_lossy());
        }
    }

    if first {
        eprintln!("no active tempdirs");
    }

    Ok(())
}

pub fn in_tempdir(tempdirs: &Path, cwd: &Path, pwd: Option<&Path>) -> Result<Option<PathBuf>> {
    let tmp = std::env::temp_dir();

    fn find_parent(path: &Path, tmp: &Path, tempdirs: &Path) -> Result<Option<PathBuf>> {
        if let Some(i) = path.parent() {
            if i == tmp || i == tempdirs {
                Ok(Some(path.to_path_buf()))
            } else {
                find_parent(i, tmp, tempdirs)
            }
        } else {
            Ok(None)
        }
    }

    if let Some(pwd) = pwd {
        for part in &pwd.canonicalize().wrap_err("canonicalize pwd")? {
            if part.to_string_lossy().starts_with(TEMPDIR_PREFIX) {
                return find_parent(pwd, &tmp, tempdirs);
            }
        }


        if pwd.starts_with(tempdirs) {
            if let Some(first) = pwd.strip_prefix(tempdirs).wrap_err("strip prefix")?.iter().next() {
                return Ok(Some(tempdirs.join(first)));
            }
        }
    }

    for part in cwd {
        if part.to_string_lossy().starts_with(TEMPDIR_PREFIX) {
            return find_parent(cwd, &tmp, tempdirs);
        }
    }

    Ok(None)
}

pub fn new_name(path: &Path) -> Result<String> {
    let mut highest_unnamed = 0;
    for i in std::fs::read_dir(path).wrap_err(format!("read {path:?}"))? {
        let i = i.wrap_err("read direntry")?;
        if let Some(rest) = i.file_name().to_string_lossy().strip_prefix("unnamed_") {
            if let Ok(i) = rest.parse::<usize>() {
                highest_unnamed = highest_unnamed.max(i);
            }
        }
    }

    loop {
        let name = format!("unnamed_{}", highest_unnamed + 1);
        if path.join(&name).exists() {
            highest_unnamed += 1;
            continue;
        }

        return Ok(name);
    }
}

pub fn delete_all(tempdirs: &Path) -> Result<PathBuf> {
    for i in std::fs::read_dir(tempdirs).wrap_err(format!("read {tempdirs:?}"))? {
        let i = i.wrap_err("read direntry")?;
        if i.metadata().wrap_err("get direntry metadata")?.is_symlink() {
            symlink::remove_symlink_auto(i.path()).wrap_err(format!("remove symlink {:?}", i.path()))?;
        }
        eprintln!("deleting {:?}", i.path());
    }


    Ok(tempdirs.to_path_buf())
}

pub fn create_tempdir(tempdirs: &Path, name: &str, cwd: &Path, pwd: Option<&Path>, symlink: bool) -> Result<PathBuf> {
    let symlink_path = tempdirs.join(name);

    if symlink_path.exists() {
        eprintln!("{:?} already exists (specify a different name)", symlink_path);
        return Ok(pwd.unwrap_or(cwd).to_path_buf());
    }

    let dir = tempdir::TempDir::new(TEMPDIR_PREFIX).wrap_err("create temp dir")?.into_path();

    Ok(if symlink {
        eprintln!("cding into {symlink_path:?}");
        symlink::symlink_auto(dir, &symlink_path).wrap_err("create symlink")?;

        symlink_path
    } else {
        eprintln!("cding into {dir:?}");

        dir
    })
}