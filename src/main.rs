use crate::archive::format::{is_archive, pack_files, read_back};
use crate::builder::compile::compile_lang;
use std::io::ErrorKind;
use std::process::Command;
use std::{self, env, fs, path::Path};

mod arch;
mod archive;
mod builder;
#[derive(PartialEq)]
enum Args {
    No,
    Yes,
}

fn help() -> ! {
    println!("you can run spec-elf with no arguments if you run directly on target dir or you can use the argumment --dir or -dir followed by the target dir");
    std::process::exit(0);
}
fn main() -> Result<(), anyhow::Error> {
    let args: Vec<String> = env::args().collect();

    // marker for arss
    let mut has_args = Args::No;
    if args.len() > 2 {
        //it has argss
        has_args = Args::Yes;
    }
    if args.len() > 1 && args[1].to_lowercase() == "--help" || args[1].to_lowercase() == "-help" || args[1].to_lowercase() == "-h" || args[1].to_lowercase() == "--h" {
        help();
    }

    let current_path = env::current_exe()?;
    let current_name = current_path.file_name().expect("current executable has no file name");

    if is_archive(&current_path)? {
        let correct_exe = read_back(&current_path)?;
        let final_file_path = env::current_dir()?.join(current_name);

        fs::write(&final_file_path, correct_exe)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&final_file_path, fs::Permissions::from_mode(0o755))?;
        }
        #[allow(clippy::zombie_processes)]
        Command::new(final_file_path).spawn()?;

        return Ok(());
    }
    if has_args == Args::Yes && (args[1].to_lowercase() == "--dir" || args[1].to_lowercase() == "-dir") && !args[2].is_empty() {
        loop {
            match env::set_current_dir(&args[2]) {
                Ok(_) => {
                    break;
                }
                Err(e) => match e.kind() {
                    ErrorKind::NotFound => {
                        println!("directory not found");
                    }
                    ErrorKind::PermissionDenied => {
                        println!("wrong permissions");
                    }
                    ErrorKind::NotADirectory => {
                        println!("this is not a dir");
                    }
                    _ => println!("idk this error"),
                },
            }
        }
    }

    let dir = env::current_dir()?;
    let dst = compile_lang(dir.to_str().expect("current directory is not valid UTF-8"))?;

    let output_path = dir.join(current_name);
    let pack_output_path = if same_path(&current_path, &output_path) { output_path.with_extension("packed") } else { output_path.clone() };

    pack_files(&current_path, &pack_output_path, &dst)?;

    if pack_output_path != output_path {
        fs::rename(&pack_output_path, &output_path)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(&output_path, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}
