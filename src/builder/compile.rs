use anyhow::{Context, Result, bail};
use std::{
    collections::HashMap,
    fs::{self, read_dir},
    path::{Path, PathBuf},
    process::Command,
};

pub const MARCH_FLAGS: [&str; 5] = ["-march=native", "-march=x86-64", "-march=x86-64-v2", "-march=x86-64-v3", "-march=x86-64-v4"];

pub const ZIG_MARCH_FLAGS: [&str; 5] = ["-mcpu=native", "-mcpu=x86_64", "-mcpu=x86_64_v2", "-mcpu=x86_64_v3", "-mcpu=x86_64_v4"];

pub const RUST_MARCH_FLAGS: [(&str, &str); 5] = [("native", "-C target-cpu=native"), ("x86_64", "-C target-cpu=x86-64"), ("x86_64_v2", "-C target-cpu=x86-64-v2"), ("x86_64_v3", "-C target-cpu=x86-64-v3"), ("x86_64_v4", "-C target-cpu=x86-64-v4")];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Idiomes {
    C,
    Cpp,
    Rust,
    Zig,
}

pub fn compile_lang(path: &str) -> Result<Vec<String>> {
    let idiome = find_idiome(path)?;

    match idiome {
        Idiomes::C => compile_c(path),
        Idiomes::Cpp => compile_cpp(path),
        Idiomes::Rust => compile_rust(path),
        Idiomes::Zig => compile_zig(path),
    }
}

fn find_idiome(path: &str) -> Result<Idiomes> {
    let project_dir = project_dir_from_path(path)?;
    let mut counts: HashMap<Idiomes, usize> = HashMap::new();

    count_languages_recursive(&project_dir, &mut counts)?;

    counts.into_iter().max_by_key(|(_, count)| *count).map(|(idiome, _)| idiome).context("could not detect project language")
}

fn count_languages_recursive(dir: &Path, counts: &mut HashMap<Idiomes, usize>) -> Result<()> {
    for entry in read_dir(dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();

        if path.is_dir() {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            if matches!(name, "target" | "build" | ".git") {
                continue;
            }

            count_languages_recursive(&path, counts)?;
            continue;
        }

        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };

        let idiome = match ext {
            "c" | "h" => Idiomes::C,
            "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Idiomes::Cpp,
            "rs" => Idiomes::Rust,
            "zig" => Idiomes::Zig,
            _ => continue,
        };

        *counts.entry(idiome).or_insert(0) += 1;
    }

    Ok(())
}

fn compile_c(path: &str) -> Result<Vec<String>> {
    let project_dir = project_dir_from_path(path)?;
    let build_dir = project_dir.join("build");
    fs::create_dir_all(&build_dir)?;

    let sources = collect_sources(&project_dir, &["c"])?;

    if sources.is_empty() {
        bail!("no C source files found");
    }

    let has_cmake = project_dir.join("CMakeLists.txt").exists();

    let mut outputs = Vec::with_capacity(MARCH_FLAGS.len());

    for march in MARCH_FLAGS {
        let march_name = march.trim_start_matches("-march=");
        let output = build_dir.join(format!("c-{march_name}"));

        if has_cmake {
            let cmake_build_dir = build_dir.join(format!("cmake-{march_name}"));
            let cmake_output_dir = build_dir.join(format!("cmake-c-out-{march_name}"));

            fs::create_dir_all(&cmake_output_dir)?;

            let status = Command::new("cmake")
                .arg("-S")
                .arg(&project_dir)
                .arg("-B")
                .arg(&cmake_build_dir)
                .arg("-DCMAKE_BUILD_TYPE=Release")
                .arg(format!("-DCMAKE_C_FLAGS_RELEASE=-O3 {march}"))
                .arg(format!("-DCMAKE_RUNTIME_OUTPUT_DIRECTORY={}", cmake_output_dir.display()))
                .arg(format!("-DCMAKE_RUNTIME_OUTPUT_DIRECTORY_RELEASE={}", cmake_output_dir.display()))
                .current_dir(&project_dir)
                .status()
                .with_context(|| format!("could not configure cmake for {march_name}"))?;

            if !status.success() {
                bail!("cmake configure failed for {march_name} with status {status}");
            }

            let status = Command::new("cmake").arg("--build").arg(&cmake_build_dir).arg("--config").arg("Release").current_dir(&project_dir).status().with_context(|| format!("could not build cmake project for {march_name}"))?;

            if !status.success() {
                bail!("cmake build failed for {march_name} with status {status}");
            }

            let built_exe = find_single_executable(&cmake_output_dir).with_context(|| format!("could not find cmake executable for {march_name}"))?;

            if output.exists() {
                fs::remove_file(&output)?;
            }

            fs::copy(&built_exe, &output)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
            }
        } else {
            let mut command = Command::new("gcc");

            command.arg("-O3").arg(march).arg("-Iinclude").arg("-Isrc");

            for source in &sources {
                command.arg(source);
            }

            let status = command.arg("-o").arg(&output).current_dir(&project_dir).status().with_context(|| format!("could not run gcc for {march_name}"))?;

            if !status.success() {
                bail!("gcc failed for {march_name} with status {status}");
            }
        }

        outputs.push(output.display().to_string());
    }

    Ok(outputs)
}
fn compile_cpp(path: &str) -> Result<Vec<String>> {
    let project_dir = project_dir_from_path(path)?;
    let build_dir = project_dir.join("build");
    fs::create_dir_all(&build_dir)?;

    let sources = collect_sources(&project_dir, &["cpp", "cc", "cxx"])?;

    if sources.is_empty() {
        bail!("no C++ source files found");
    }

    let has_cmake = project_dir.join("CMakeLists.txt").is_file();

    let mut outputs = Vec::with_capacity(MARCH_FLAGS.len());

    for march in MARCH_FLAGS {
        let march_name = march.trim_start_matches("-march=");
        let output = build_dir.join(format!("cpp-{march_name}"));

        if has_cmake {
            let cmake_build_dir = build_dir.join(format!("cmake-cpp-{march_name}"));
            let cmake_output_dir = build_dir.join(format!("cmake-cpp-out-{march_name}"));

            fs::create_dir_all(&cmake_output_dir)?;

            let status = Command::new("cmake")
                .arg("-S")
                .arg(&project_dir)
                .arg("-B")
                .arg(&cmake_build_dir)
                .arg("-DCMAKE_BUILD_TYPE=Release")
                .arg(format!("-DCMAKE_CXX_FLAGS_RELEASE=-O3 {march}"))
                .arg(format!("-DCMAKE_RUNTIME_OUTPUT_DIRECTORY={}", cmake_output_dir.display()))
                .arg(format!("-DCMAKE_RUNTIME_OUTPUT_DIRECTORY_RELEASE={}", cmake_output_dir.display()))
                .current_dir(&project_dir)
                .status()
                .with_context(|| format!("could not configure cmake for {march_name}"))?;

            if !status.success() {
                bail!("cmake configure failed for {march_name} with status {status}");
            }

            let status = Command::new("cmake").arg("--build").arg(&cmake_build_dir).arg("--config").arg("Release").current_dir(&project_dir).status().with_context(|| format!("could not build cmake project for {march_name}"))?;

            if !status.success() {
                bail!("cmake build failed for {march_name} with status {status}");
            }

            let built_exe = find_single_executable(&cmake_output_dir).with_context(|| format!("could not find cmake executable for {march_name}"))?;

            if output.exists() {
                fs::remove_file(&output)?;
            }

            fs::copy(&built_exe, &output)?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                fs::set_permissions(&output, fs::Permissions::from_mode(0o755))?;
            }
        } else {
            let mut command = Command::new("g++");

            command.arg("-O3").arg(march).arg("-I.").arg("-Iinclude").arg("-Isrc");

            for source in &sources {
                command.arg(source);
            }

            let status = command.arg("-o").arg(&output).current_dir(&project_dir).status().with_context(|| format!("could not run g++ for {march_name}"))?;

            if !status.success() {
                bail!("g++ failed for {march_name} with status {status}");
            }
        }

        outputs.push(output.display().to_string());
    }

    Ok(outputs)
}

fn compile_zig(path: &str) -> Result<Vec<String>> {
    let project_dir = project_dir_from_path(path)?;
    let build_dir = project_dir.join("build");
    fs::create_dir_all(&build_dir)?;

    let source = if Path::new(path).is_file() { PathBuf::from(path) } else { find_first_source(&project_dir, &["zig"])? };

    let mut outputs = Vec::with_capacity(ZIG_MARCH_FLAGS.len());

    for march in ZIG_MARCH_FLAGS {
        let name = march.trim_start_matches("-mcpu=");
        let output = build_dir.join(format!("zig-{name}"));
        let emit = format!("-femit-bin={}", output.display());

        let status = Command::new("zig").arg("build-exe").arg(&source).arg("-O").arg("ReleaseFast").arg(march).arg(&emit).current_dir(&project_dir).status().with_context(|| format!("could not run zig for {name}"))?;

        if !status.success() {
            bail!("zig failed for {name} with status {status}");
        }

        outputs.push(output.display().to_string());
    }

    Ok(outputs)
}

pub fn compile_rust(path: &str) -> Result<Vec<String>> {
    let project_dir = find_cargo_project_dir(path)?;
    let build_dir = project_dir.join("build");
    fs::create_dir_all(&build_dir)?;

    let package_name = cargo_package_name(&project_dir)?;

    let mut outputs = Vec::with_capacity(RUST_MARCH_FLAGS.len());

    for (name, rustflags) in RUST_MARCH_FLAGS {
        let target_dir = project_dir.join("target").join(format!("rust-{name}"));
        let output = build_dir.join(format!("rust-{name}"));

        let status = Command::new("cargo").args(["build", "--release"]).current_dir(&project_dir).env("RUSTFLAGS", rustflags).env("CARGO_TARGET_DIR", &target_dir).status().with_context(|| format!("failed to run cargo for {name}"))?;

        if !status.success() {
            bail!("cargo failed for {name} with status {status}");
        }

        let built_bin = target_dir.join("release").join(&package_name);

        fs::copy(&built_bin, &output).with_context(|| format!("failed to copy built binary from {} to {}", built_bin.display(), output.display()))?;

        outputs.push(output.display().to_string());
    }

    Ok(outputs)
}

fn project_dir_from_path(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);

    if path.is_file() {
        return path.parent().map(Path::to_path_buf).context("file path has no parent directory");
    }

    Ok(path.to_path_buf())
}

fn find_cargo_project_dir(path: &str) -> Result<PathBuf> {
    let mut dir = project_dir_from_path(path)?;

    loop {
        if dir.join("Cargo.toml").is_file() {
            return Ok(dir);
        }

        if !dir.pop() {
            bail!("could not find Cargo.toml");
        }
    }
}

fn cargo_package_name(project_dir: &Path) -> Result<String> {
    let cargo_toml_path = project_dir.join("Cargo.toml");
    let cargo_toml = fs::read_to_string(&cargo_toml_path).with_context(|| format!("failed to read {}", cargo_toml_path.display()))?;

    for line in cargo_toml.lines() {
        let line = line.trim();

        if let Some(name) = line.strip_prefix("name = ") {
            return Ok(name.trim_matches('"').to_string());
        }
    }

    bail!("could not find package name in Cargo.toml");
}

fn collect_sources(project_dir: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut sources = Vec::new();
    collect_sources_recursive(project_dir, extensions, &mut sources)?;
    Ok(sources)
}

fn collect_sources_recursive(dir: &Path, extensions: &[&str], sources: &mut Vec<PathBuf>) -> Result<()> {
    for entry in read_dir(dir)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();

        if path.is_dir() {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            if matches!(name, "target" | "build" | ".git") {
                continue;
            }

            collect_sources_recursive(&path, extensions, sources)?;
            continue;
        }

        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            continue;
        };

        if extensions.contains(&ext) {
            sources.push(path);
        }
    }

    Ok(())
}

fn find_single_executable(dir: &Path) -> Result<PathBuf> {
    let mut built_exe = None;

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();

        if !path.is_file() {
            continue;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = path.metadata()?.permissions().mode();

            if mode & 0o111 == 0 {
                continue;
            }
        }

        if built_exe.is_some() {
            bail!("cmake produced multiple executables in {}", dir.display());
        }

        built_exe = Some(path);
    }

    built_exe.with_context(|| format!("cmake produced no executable in {}", dir.display()))
}

fn find_first_source(project_dir: &Path, extensions: &[&str]) -> Result<PathBuf> {
    let sources = collect_sources(project_dir, extensions)?;

    sources.into_iter().next().context("could not find source file")
}
