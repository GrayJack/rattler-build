//! Functions to collect environment variables that are used during the build process.
use std::path::{Path, PathBuf};
use std::{collections::HashMap, env};

use rattler_conda_types::Platform;

use crate::linux;
use crate::macos;
use crate::metadata::Output;
use crate::unix;
use crate::windows;

fn get_stdlib_dir(prefix: &Path, platform: &Platform, py_ver: &str) -> PathBuf {
    if platform.is_windows() {
        prefix.join("Lib")
    } else {
        let lib_dir = prefix.join("lib");
        lib_dir.join(format!("python{}", py_ver))
    }
}

fn get_sitepackages_dir(prefix: &Path, platform: &Platform, py_ver: &str) -> PathBuf {
    get_stdlib_dir(prefix, platform, py_ver).join("site-packages")
}

/// Returns a map of environment variables for Python that are used in the build process.
///
/// Variables:
/// - PYTHON: path to Python executable
/// - PY3K: 1 if Python 3, 0 if Python 2
/// - PY_VER: Python version (major.minor), e.g. 3.8
/// - STDLIB_DIR: Python standard library directory
/// - SP_DIR: Python site-packages directory
/// - NPY_VER: Numpy version (major.minor), e.g. 1.19
/// - NPY_DISTUTILS_APPEND_FLAGS: 1 (https://github.com/conda/conda-build/pull/3015)
pub fn python_vars(output: &Output) -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();

    if output.host_platform().is_windows() {
        let python = output.prefix().join("python.exe");
        result.insert("PYTHON".to_string(), python.to_string_lossy().to_string());
    } else {
        let python = output.prefix().join("bin/python");
        result.insert("PYTHON".to_string(), python.to_string_lossy().to_string());
    }

    // find python in the host dependencies
    let mut python_version = output.variant().get("python").map(|s| s.to_string());
    if python_version.is_none() {
        if let Some((record, requested)) = output.find_resolved_package("python") {
            if requested {
                python_version = Some(record.package_record.version.to_string());
            }
        }
    }

    if let Some(py_ver) = python_version {
        let py_ver = py_ver.split('.').collect::<Vec<_>>();
        let py_ver_str = format!("{}.{}", py_ver[0], py_ver[1]);
        let stdlib_dir = get_stdlib_dir(output.prefix(), output.host_platform(), &py_ver_str);
        let site_packages_dir =
            get_sitepackages_dir(output.prefix(), output.host_platform(), &py_ver_str);
        result.insert(
            "PY3K".to_string(),
            if py_ver[0] == "3" {
                "1".to_string()
            } else {
                "0".to_string()
            },
        );
        result.insert("PY_VER".to_string(), py_ver_str);
        result.insert(
            "STDLIB_DIR".to_string(),
            stdlib_dir.to_string_lossy().to_string(),
        );
        result.insert(
            "SP_DIR".to_string(),
            site_packages_dir.to_string_lossy().to_string(),
        );
    }

    if let Some(npy_version) = output.variant().get("numpy") {
        let np_ver = npy_version.split('.').collect::<Vec<_>>();
        let np_ver = format!("{}.{}", np_ver[0], np_ver[1]);

        result.insert("NPY_VER".to_string(), np_ver);
    }
    result.insert("NPY_DISTUTILS_APPEND_FLAGS".to_string(), "1".to_string());

    result
}

/// Returns a map of environment variables for R that are used in the build process.
///
/// Variables:
/// - R_VER: R version (major.minor), e.g. 4.0
/// - R: Path to R executable
/// - R_USER: Path to R user directory
///
pub fn r_vars(output: &Output) -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();

    if let Some(r_ver) = output.variant().get("r-base") {
        result.insert("R_VER".to_string(), r_ver.clone());

        let r_bin = if output.host_platform().is_windows() {
            output.prefix().join("Scripts/R.exe")
        } else {
            output.prefix().join("bin/R")
        };

        let r_user = output.prefix().join("Libs/R");

        result.insert("R".to_string(), r_bin.to_string_lossy().to_string());
        result.insert("R_USER".to_string(), r_user.to_string_lossy().to_string());
    }

    result
}

pub fn language_vars(output: &Output) -> HashMap<String, String> {
    let mut result = HashMap::<String, String>::new();

    result.extend(python_vars(output));
    result.extend(r_vars(output));

    result
}

/// Returns a map of environment variables that are used in the build process.
/// Also adds platform-specific variables.
///
/// Variables:
/// - CPU_COUNT: Number of CPUs
/// - SHLIB_EXT: Shared library extension for platform (e.g. Linux -> .so, Windows -> .dll, macOS -> .dylib)
///
/// Forwards the following environment variables:
/// - PATH: Path where executables are found
/// - LANG: Language (e.g. en_US.UTF-8)
/// - LC_ALL: Language (e.g. en_US.UTF-8)
/// - MAKEFLAGS: Make flags (e.g. -j4)
pub fn os_vars(prefix: &Path, platform: &Platform) -> HashMap<String, String> {
    let mut vars = HashMap::<String, String>::new();

    let path_var = if platform.is_windows() {
        "Path"
    } else {
        "PATH"
    };

    vars.insert(
        "CPU_COUNT".to_string(),
        env::var("CPU_COUNT").unwrap_or_else(|_| num_cpus::get().to_string()),
    );
    vars.insert("LANG".to_string(), env::var("LANG").unwrap_or_default());
    vars.insert("LC_ALL".to_string(), env::var("LC_ALL").unwrap_or_default());
    vars.insert(
        "MAKEFLAGS".to_string(),
        env::var("MAKEFLAGS").unwrap_or_default(),
    );

    let shlib_ext = if platform.is_windows() {
        ".dll".to_string()
    } else if platform.is_osx() {
        ".dylib".to_string()
    } else if platform.is_linux() {
        ".so".to_string()
    } else {
        ".not_implemented".to_string()
    };

    vars.insert("SHLIB_EXT".to_string(), shlib_ext);
    if let Ok(path) = env::var(path_var) {
        vars.insert(path_var.to_string(), path);
    }

    if cfg!(target_family = "windows") {
        vars.extend(windows::env::default_env_vars(prefix, platform));
    } else if cfg!(target_family = "unix") {
        vars.extend(unix::env::default_env_vars(prefix));
    }

    if platform.is_osx() {
        vars.extend(macos::env::default_env_vars(prefix, platform));
    } else if platform.is_linux() {
        vars.extend(linux::env::default_env_vars(prefix, platform));
    }

    vars
}

macro_rules! insert {
    ($map:expr, $key:expr, $value:expr) => {
        $map.insert($key.to_string(), $value.to_string());
    };
}

/// Set environment variables that help to force color output.
fn force_color_vars() -> HashMap<String, String> {
    let mut vars = HashMap::<String, String>::new();

    insert!(vars, "CLICOLOR_FORCE", "1");
    insert!(vars, "FORCE_COLOR", "1");
    insert!(vars, "AM_COLOR_TESTS", "always");
    insert!(vars, "MAKE_TERMOUT", "1");
    insert!(vars, "CMAKE_COLOR_DIAGNOSTICS", "ON");

    insert!(
        vars,
        "GCC_COLORS",
        "error=01;31:warning=01;35:note=01;36:caret=01;32:locus=01:quote=01"
    );

    vars
}

/// Return all variables that should be set during the build process, including
/// operating system specific environment variables.
pub fn vars(output: &Output, build_state: &str) -> HashMap<String, String> {
    let mut vars = HashMap::<String, String>::new();

    insert!(vars, "CONDA_BUILD", "1");
    insert!(vars, "PYTHONNOUSERSITE", "1");

    if let Some((_, host_arch)) = output.host_platform().to_string().rsplit_once('-') {
        insert!(vars, "ARCH", host_arch);
    }

    let directories = &output.build_configuration.directories;
    insert!(
        vars,
        "CONDA_DEFAULT_ENV",
        directories.host_prefix.to_string_lossy()
    );
    insert!(vars, "PREFIX", directories.host_prefix.to_string_lossy());
    insert!(
        vars,
        "BUILD_PREFIX",
        directories.build_prefix.to_string_lossy()
    );
    insert!(vars, "RECIPE_DIR", directories.recipe_dir.to_string_lossy());
    insert!(vars, "SRC_DIR", directories.work_dir.to_string_lossy());
    insert!(vars, "BUILD_DIR", directories.build_dir.to_string_lossy());

    // python variables
    // hard-code this because we never want pip's build isolation
    // https://github.com/conda/conda-build/pull/2972#discussion_r198290241
    //
    // Note that pip env "NO" variables are inverted logic.
    //    PIP_NO_BUILD_ISOLATION=False means don't use build isolation.
    insert!(vars, "PIP_NO_BUILD_ISOLATION", "False");
    // Some other env vars to have pip ignore dependencies. We supply them ourselves instead.
    insert!(vars, "PIP_NO_DEPENDENCIES", "True");
    insert!(vars, "PIP_IGNORE_INSTALLED", "True");

    // pip's cache directory (PIP_NO_CACHE_DIR) should not be
    // disabled as this results in .egg-info rather than
    // .dist-info directories being created, see gh-3094
    // set PIP_CACHE_DIR to a path in the work dir that does not exist.
    if let Some(work_dir_parent) = directories.work_dir.parent() {
        let pip_cache = work_dir_parent.join("pip_cache");
        insert!(vars, "PIP_CACHE_DIR", pip_cache.to_string_lossy());
    }

    // tell pip to not get anything from PyPI, please. We have everything we need
    // locally, and if we don't, it's a problem.
    insert!(vars, "PIP_NO_INDEX", "True");

    // For noarch packages, do not write any bytecode
    if output.build_configuration.target_platform == Platform::NoArch {
        insert!(vars, "PYTHONDONTWRITEBYTECODE", "1");
    }

    if output.build_configuration.force_colors {
        vars.extend(force_color_vars());
    }

    // pkg vars
    insert!(vars, "PKG_NAME", output.name().as_normalized());
    insert!(vars, "PKG_VERSION", output.version());
    insert!(
        vars,
        "PKG_BUILDNUM",
        output.recipe.build().number().to_string()
    );

    let hash = output.build_configuration.hash.clone();
    insert!(
        vars,
        "PKG_BUILD_STRING",
        output
            .recipe
            .build()
            .string()
            .resolve(&hash, output.recipe.build().number)
            .into_owned()
    );
    insert!(vars, "PKG_HASH", hash);

    if output.build_configuration.cross_compilation() {
        insert!(vars, "CONDA_BUILD_CROSS_COMPILATION", "1");
    } else {
        insert!(vars, "CONDA_BUILD_CROSS_COMPILATION", "0");
    }
    insert!(vars, "SUBDIR", output.target_platform().to_string());
    insert!(
        vars,
        "build_platform",
        output.build_configuration.build_platform.to_string()
    );
    insert!(
        vars,
        "target_platform",
        output.target_platform().to_string()
    );
    insert!(vars, "host_platform", output.host_platform().to_string());
    insert!(vars, "CONDA_BUILD_STATE", build_state);

    vars.extend(language_vars(output));

    // for reproducibility purposes, set the SOURCE_DATE_EPOCH to the configured timestamp
    // this value will be taken from the previous package for rebuild purposes
    let timestamp_epoch_secs = output.build_configuration.timestamp.timestamp();
    vars.insert(
        "SOURCE_DATE_EPOCH".to_string(),
        timestamp_epoch_secs.to_string(),
    );

    vars
}
