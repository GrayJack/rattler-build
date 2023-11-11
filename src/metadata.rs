//! All the metadata that makes up a recipe file
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display, Formatter},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use chrono::{DateTime, Utc};
use rattler_conda_types::{package::ArchiveType, PackageName, Platform};
use serde::{Deserialize, Serialize};

use crate::{
    recipe::parser::Dependency,
    render::resolved_dependencies::{apply_variant, FinalizedDependencies},
};

pub struct Metadata {
    pub name: String,
    pub version: String,
    pub requirements: Vec<String>,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            name: String::from(""),
            version: String::from("0.0.0"),
            requirements: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GitRev(String);

impl FromStr for GitRev {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(GitRev(s.to_string()))
    }
}
impl Default for GitRev {
    fn default() -> Self {
        Self(String::from("HEAD"))
    }
}
impl Display for GitRev {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Directories used during the build process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directories {
    /// The directory where the recipe is located
    #[serde(skip)]
    pub recipe_dir: PathBuf,
    /// The host prefix is the directory where host dependencies are installed
    /// Exposed as `$PREFIX` (or `%PREFIX%` on Windows) in the build script
    pub host_prefix: PathBuf,
    /// The build prefix is the directory where build dependencies are installed
    /// Exposed as `$BUILD_PREFIX` (or `%BUILD_PREFIX%` on Windows) in the build script
    pub build_prefix: PathBuf,
    /// The work directory is the directory where the source code is copied to
    pub work_dir: PathBuf,
    /// The parent directory of host, build and work directories
    pub build_dir: PathBuf,
    /// The output directory or local channel directory
    #[serde(skip)]
    pub output_dir: PathBuf,
}

fn setup_build_dir(
    output_dir: &Path,
    name: &str,
    no_build_id: bool,
    timestamp: &DateTime<Utc>,
) -> Result<PathBuf, std::io::Error> {
    let since_the_epoch = timestamp.timestamp();

    let dirname = if no_build_id {
        format!("rattler-build_{}", name)
    } else {
        format!("rattler-build_{}_{:?}", name, since_the_epoch)
    };
    let path = output_dir.join("bld").join(dirname);
    fs::create_dir_all(path.join("work"))?;
    Ok(path)
}

impl Directories {
    pub fn create(
        name: &str,
        recipe_path: &Path,
        output_dir: &Path,
        no_build_id: bool,
        timestamp: &DateTime<Utc>,
    ) -> Result<Directories, std::io::Error> {
        if !output_dir.exists() {
            fs::create_dir(output_dir)?;
        }
        let output_dir = fs::canonicalize(output_dir)?;

        let build_dir = setup_build_dir(&output_dir, name, no_build_id, timestamp)
            .expect("Could not create build directory");
        let recipe_dir = recipe_path.parent().unwrap().to_path_buf();

        let host_prefix = if cfg!(target_os = "windows") {
            build_dir.join("h_env")
        } else {
            let placeholder_template = "_placehold";
            let mut placeholder = String::new();
            let placeholder_length: usize = 255;

            while placeholder.len() < placeholder_length {
                placeholder.push_str(placeholder_template);
            }

            let placeholder = placeholder
                [0..placeholder_length - build_dir.join("host_env").as_os_str().len()]
                .to_string();

            build_dir.join(format!("host_env{}", placeholder))
        };

        let directories = Directories {
            build_dir: build_dir.clone(),
            build_prefix: build_dir.join("build_env"),
            host_prefix,
            work_dir: build_dir.join("work"),
            recipe_dir,
            output_dir,
        };

        Ok(directories)
    }

    /// create all directories
    pub fn recreate_directories(&self) -> Result<(), std::io::Error> {
        if self.build_dir.exists() {
            fs::remove_dir_all(&self.build_dir).unwrap();
        }

        if !self.output_dir.exists() {
            fs::create_dir(&self.output_dir)?;
        }

        fs::create_dir_all(&self.build_dir)?;
        fs::create_dir_all(&self.work_dir)?;
        fs::create_dir_all(&self.build_prefix)?;
        fs::create_dir_all(&self.host_prefix)?;

        Ok(())
    }
}

/// Default value for store recipe for backwards compatiblity
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildConfiguration {
    /// The target platform for the build
    pub target_platform: Platform,
    /// The host platform (usually target platform, but for `noarch` it's the build platform)
    pub host_platform: Platform,
    /// The build platform (the platform that the build is running on)
    pub build_platform: Platform,
    /// The selected variant for this build
    pub variant: BTreeMap<String, String>,
    /// THe computed hash of the variant
    pub hash: String,
    /// The directories for the build (work, source, build, host, ...)
    pub directories: Directories,
    /// The channels to use when resolving environments
    pub channels: Vec<String>,
    /// The timestamp to use for the build
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// All subpackages coming from this output or other outputs from the same recipe
    pub subpackages: BTreeMap<PackageName, PackageIdentifier>,
    /// Package format (.tar.bz2 or .conda)
    pub package_format: ArchiveType,
    /// Wether to store the recipe and build instructions in the final package or not
    #[serde(skip_serializing, default = "default_true")]
    pub store_recipe: bool,
}

impl BuildConfiguration {
    /// true if the build is cross-compiling
    pub fn cross_compilation(&self) -> bool {
        self.target_platform != self.build_platform
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PackageIdentifier {
    pub name: PackageName,
    pub version: String,
    pub build_string: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
    pub recipe: crate::recipe::parser::Recipe,
    pub build_configuration: BuildConfiguration,
    pub finalized_dependencies: Option<FinalizedDependencies>,
}

impl Output {
    /// The name of the package
    pub fn name(&self) -> &PackageName {
        self.recipe.package().name()
    }

    /// The version of the package
    pub fn version(&self) -> &str {
        self.recipe.package().version()
    }

    /// The build string is usually set automatically as the hash of the variant configuration.
    pub fn build_string(&self) -> &str {
        self.recipe.build().string().as_ref().unwrap()
    }

    /// retrieve an identifier for this output ({name}-{version}-{build_string})
    pub fn identifier(&self) -> String {
        format!(
            "{}-{}-{}",
            self.name().as_normalized(),
            self.version(),
            self.build_string()
        )
    }

    /// Retrieve a iterator over all the dependencies of this output
    pub fn dependencies(&self) -> impl Iterator<Item = &Dependency> {
        self.recipe.requirements().all()
    }

    /// Retrieve a iterator over all the build dependencies of this output
    pub fn build_dependencies(&self) -> impl Iterator<Item = &Dependency> {
        self.recipe.requirements().all_build_time()
    }
}

impl Display for Output {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\nOutput: {}-{}-{}\n",
            self.name().as_normalized(),
            self.version(),
            self.build_string()
        )?;

        // make a table of the variant configuration
        writeln!(f, "Variant configuration:")?;

        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::UTF8_FULL)
            .set_header(vec!["Variant", "Version"]);

        self.build_configuration.variant.iter().for_each(|(k, v)| {
            table.add_row(vec![k, v]);
        });

        writeln!(f, "{}\n", table)?;

        if let Some(finalized_dependencies) = &self.finalized_dependencies {
            // create a table with the finalized dependencies
            if let Some(host) = &finalized_dependencies.build {
                writeln!(f, "Build dependencies:")?;
                writeln!(f, "{}\n", host)?;
            }

            if let Some(host) = &finalized_dependencies.host {
                writeln!(f, "Host dependencies:")?;
                writeln!(f, "{}\n", host)?;
            }

            if !finalized_dependencies.run.depends.is_empty() {
                writeln!(f, "Run dependencies:")?;
                let mut table = comfy_table::Table::new();
                table
                    .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                    .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
                    .set_header(vec!["Name", "Spec"]);

                finalized_dependencies.run.depends.iter().for_each(|d| {
                    let rendered = d.render();
                    table.add_row(rendered.splitn(2, ' ').collect::<Vec<&str>>());
                });

                writeln!(f, "{}\n", table)?;
            }

            if !finalized_dependencies.run.constrains.is_empty() {
                writeln!(f, "Run constraints:")?;
                let mut table = comfy_table::Table::new();
                table
                    .load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
                    .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
                    .set_header(vec!["Name", "Spec"]);

                finalized_dependencies.run.constrains.iter().for_each(|d| {
                    let rendered = d.render();
                    table.add_row(rendered.splitn(2, ' ').collect::<Vec<&str>>());
                });

                writeln!(f, "{}\n", table)?;
            }
        }
        writeln!(f, "\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PackageMeta {
    name: PackageName,
    variant: BTreeMap<String, String>,
}

impl Output {
    /// Returns the package metadata for this output
    fn package_meta(&self) -> PackageMeta {
        PackageMeta {
            name: self.name().clone(),
            variant: self.build_configuration.variant.clone(),
        }
    }
}

/// Helper enum to model the recursion inside `get_topological_order` as iteration, to support
/// dependency graphs of arbitrary depth without causing stack overflows
#[derive(Debug, Clone)]
enum Action {
    ResolveAndBuild(PackageMeta),
    Build(PackageMeta),
}

/// Sorts the packages topologically
///
/// This function is deterministic, meaning that it will return the same result regardless of the
/// order of `packages` and of the `depends` vector inside the records.
///
/// If cycles are encountered, and one of the packages in the cycle is noarch, the noarch package
/// is sorted _after_ the other packages in the cycle. This is done to ensure that the noarch
/// package is built last, so that it can be linked correctly (ie. compiled with Python if
/// necessary).
///
/// Note that this function only works for packages with unique names.
pub fn topological_sort(packages: Vec<Output>) -> Vec<Output> {
    let roots = get_roots(&packages, None);

    let mut all_packages = packages
        .iter()
        .map(|p| (p.package_meta(), p))
        .collect::<BTreeMap<_, _>>();

    // detect cycles
    let mut visited = BTreeSet::new();
    let mut stack = Vec::new();
    let mut cycles = Vec::new();

    for root in &roots {
        if !visited.contains(root) {
            if let Some(cycle) = find_cycles(root, &all_packages, &mut visited, &mut stack) {
                cycles.push(cycle);
            }
        }
    }

    // print all cycles
    for cycle in &cycles {
        tracing::debug!("Found cycle: {:?}", cycle);
    }

    // Break cycles
    let cycle_breaks = break_cycles(cycles, &all_packages);

    // obtain the new roots (packages that are not dependencies of any other package)
    // this is needed because breaking cycles can create new roots
    let roots = get_roots(&packages, Some(&cycle_breaks));

    get_topological_order(roots, &mut all_packages, &cycle_breaks)
}

fn get_roots(
    packages: &[Output],
    cycle_breaks: Option<&BTreeSet<(PackageMeta, PackageMeta)>>,
) -> Vec<PackageMeta> {
    let all_packages: BTreeSet<_> = packages.iter().map(|p| p.package_meta()).collect();

    let dependencies: BTreeSet<_> = packages
        .iter()
        .flat_map(|p| {
            let dependencies: Vec<_> = p.build_dependencies().cloned().collect();
            let dependencies = apply_variant(&dependencies, &p.build_configuration).unwrap();

            dependencies
                .into_iter()
                .map(|dep| {
                    let name = dep.spec().name.clone().unwrap();
                    PackageMeta {
                        name,
                        variant: p.build_configuration.variant.clone(),
                    }
                })
                .filter(|dep| {
                    // filter out circular dependencies
                    if let Some(cycle_breaks) = cycle_breaks {
                        !cycle_breaks.contains(&(p.package_meta(), dep.clone()))
                    } else {
                        true
                    }
                })
        })
        .collect();

    all_packages.difference(&dependencies).cloned().collect()
}

/// Find cycles with DFS
fn find_cycles(
    node: &PackageMeta,
    packages: &BTreeMap<PackageMeta, &Output>,
    visited: &mut BTreeSet<PackageMeta>,
    stack: &mut Vec<PackageMeta>,
) -> Option<Vec<PackageMeta>> {
    visited.insert(node.clone());
    stack.push(node.clone());

    if let Some(package) = packages.get(node) {
        let dependencies: Vec<_> = package.dependencies().cloned().collect();
        let dependencies = apply_variant(&dependencies, &package.build_configuration).unwrap();
        let dependencies = dependencies.into_iter().map(|dep| {
            let name = dep.spec().name.clone().unwrap();
            PackageMeta {
                name,
                variant: package.build_configuration.variant.clone(),
            }
        });

        // let dependencies = apply_variant(&dependencies, &package.build_configuration).unwrap();
        // .chain(package. .pins.iter().map(|x| &x.0));

        for dependency in dependencies {
            if !visited.contains(&dependency) {
                if let Some(cycle) = find_cycles(&dependency, packages, visited, stack) {
                    return Some(cycle);
                }
            } else if stack.contains(&dependency) {
                // Cycle detected. We clone the part of the stack that forms the cycle.
                if let Some(pos) = stack.iter().position(|x| x == &dependency) {
                    return Some(stack[pos..].to_vec());
                }
            }
        }
    }

    stack.pop();
    None
}

/// Breaks cycles by removing the edges that form them
/// Edges from arch to noarch packages are removed to break the cycles.
fn break_cycles(
    cycles: Vec<Vec<PackageMeta>>,
    packages: &BTreeMap<PackageMeta, &Output>,
) -> BTreeSet<(PackageMeta, PackageMeta)> {
    // we record the edges that we want to remove
    let mut cycle_breaks = BTreeSet::default();

    for cycle in cycles {
        for i in 0..cycle.len() {
            let pi1 = &cycle[i];
            // Next package in cycle, wraps around
            let pi2 = &cycle[(i + 1) % cycle.len()];

            let p1 = &packages[pi1];
            let p2 = &packages[pi2];

            // prefer arch packages over noarch packages
            let p1_noarch = p1.build_configuration.build_platform.arch().is_none();
            let p2_noarch = p2.build_configuration.build_platform.arch().is_none();
            if p1_noarch && !p2_noarch {
                cycle_breaks.insert((pi1.clone(), pi2.clone()));
                break;
            } else if !p1_noarch && p2_noarch {
                cycle_breaks.insert((pi2.clone(), pi1.clone()));
                break;
            }
        }
    }
    tracing::debug!("Breaking cycle: {:?}", cycle_breaks);
    cycle_breaks
}

/// Returns a vector containing the topological ordering of the packages, based on the provided
/// roots
fn get_topological_order(
    mut roots: Vec<PackageMeta>,
    packages: &mut BTreeMap<PackageMeta, &Output>,
    cycle_breaks: &BTreeSet<(PackageMeta, PackageMeta)>,
) -> Vec<Output> {
    // Sorting makes this step deterministic (i.e. the same output is returned, regardless of the
    // original order of the input)
    roots.sort();

    // Store the name of each package in `order` according to the graph's topological sort
    let mut order = Vec::new();
    let mut visited_packages = BTreeSet::default();
    let mut stack: Vec<_> = roots.into_iter().map(Action::ResolveAndBuild).collect();
    while let Some(action) = stack.pop() {
        match action {
            Action::Build(package_name) => {
                order.push(package_name);
            }
            Action::ResolveAndBuild(package_name) => {
                let already_visited = !visited_packages.insert(package_name.clone());
                if already_visited {
                    continue;
                }

                let mut deps: Vec<_> = match &packages.get(&package_name) {
                    Some(p) => {
                        let dependencies: Vec<_> = p.build_dependencies().cloned().collect();
                        let dependencies =
                            apply_variant(&dependencies, &p.build_configuration).unwrap();
                        dependencies
                            .into_iter()
                            .map(|dep| {
                                let name = dep.spec().name.clone().unwrap();
                                PackageMeta {
                                    name,
                                    variant: p.build_configuration.variant.clone(),
                                }
                            })
                            .collect()
                    }
                    None => {
                        // This is a virtual package, so no real package was found for it
                        continue;
                    }
                };

                // Remove the edges that form cycles
                deps.retain(|dep| !cycle_breaks.contains(&(package_name.clone(), dep.clone())));

                // Sorting makes this step deterministic (i.e. the same output is returned, regardless of the
                // original order of the input)
                deps.sort();

                // Install dependencies, then ourselves (the order is reversed because of the stack)
                stack.push(Action::Build(package_name));
                stack.extend(deps.into_iter().map(Action::ResolveAndBuild));
            }
        }
    }

    // Apply the order we just obtained
    let mut output = Vec::with_capacity(order.len());
    for name in order {
        let package = packages.remove(&name).unwrap();
        output.push(package.clone());
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_build_dir_test() {
        // without build_id (aka timestamp)
        let dir = tempfile::tempdir().unwrap();
        let p1 = setup_build_dir(dir.path(), "name", true, &Utc::now()).unwrap();
        let f1 = p1.file_name().unwrap();
        assert!(f1.eq("rattler-build_name"));
        _ = std::fs::remove_dir_all(p1);

        // with build_id (aka timestamp)
        let timestamp = &Utc::now();
        let p2 = setup_build_dir(dir.path(), "name", false, timestamp).unwrap();
        let f2 = p2.file_name().unwrap().to_string_lossy();
        let epoch = timestamp.timestamp();
        assert!(f2.eq(&format!("rattler-build_name_{epoch}")));
        _ = std::fs::remove_dir_all(p2);
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use chrono::TimeZone;
    use insta::assert_yaml_snapshot;
    use rattler_conda_types::{
        MatchSpec, NoArchType, PackageName, PackageRecord, RepoDataRecord, VersionWithSource,
    };
    use rattler_digest::{parse_digest_from_hex, Md5, Sha256};
    use url::Url;

    use crate::render::resolved_dependencies::{self, DependencyInfo};

    use super::{Directories, Output};

    #[test]
    fn test_directories_yaml_rendering() {
        let tempdir = tempfile::tempdir().unwrap();

        let directories = Directories::create(
            "name",
            &tempdir.path().join("recipe"),
            &tempdir.path().join("output"),
            false,
            &chrono::Utc::now(),
        )
        .unwrap();

        // test yaml roundtrip
        let yaml = serde_yaml::to_string(&directories).unwrap();
        let directories2: Directories = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(directories.build_dir, directories2.build_dir);
        assert_eq!(directories.build_prefix, directories2.build_prefix);
        assert_eq!(directories.host_prefix, directories2.host_prefix);
    }

    #[test]
    fn test_resolved_dependencies_rendering() {
        let resolved_dependencies = resolved_dependencies::ResolvedDependencies {
            specs: vec![DependencyInfo::Raw {
                spec: MatchSpec::from_str("python 3.12.* h12332").unwrap(),
            }],
            resolved: vec![RepoDataRecord {
                package_record: PackageRecord {
                    arch: Some("x86_64".into()),
                    build: "h123".into(),
                    build_number: 0,
                    constrains: vec![],
                    depends: vec![],
                    features: None,
                    legacy_bz2_md5: None,
                    legacy_bz2_size: None,
                    license: Some("MIT".into()),
                    license_family: None,
                    md5: parse_digest_from_hex::<Md5>("68b329da9893e34099c7d8ad5cb9c940"),
                    name: PackageName::from_str("test").unwrap(),
                    noarch: NoArchType::none(),
                    platform: Some("linux".into()),
                    sha256: parse_digest_from_hex::<Sha256>(
                        "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                    ),
                    size: Some(123123),
                    subdir: "linux-64".into(),
                    timestamp: Some(chrono::Utc.timestamp_opt(123123, 0).unwrap()),
                    track_features: vec![],
                    version: VersionWithSource::from_str("1.2.3").unwrap(),
                },
                file_name: "test-1.2.3-h123.tar.bz2".into(),
                url: Url::from_str("https://test.com/test/linux-64/test-1.2.3-h123.tar.bz2")
                    .unwrap(),
                channel: "test".into(),
            }],
            run_exports: Default::default(),
        };

        // test yaml roundtrip
        assert_yaml_snapshot!(resolved_dependencies);
        let yaml = serde_yaml::to_string(&resolved_dependencies).unwrap();
        let resolved_dependencies2: resolved_dependencies::ResolvedDependencies =
            serde_yaml::from_str(&yaml).unwrap();
        let yaml2 = serde_yaml::to_string(&resolved_dependencies2).unwrap();
        assert_eq!(yaml, yaml2);

        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");
        let yaml3 = std::fs::read_to_string(test_data_dir.join("dependencies.yaml")).unwrap();
        let parsed_yaml3: resolved_dependencies::ResolvedDependencies =
            serde_yaml::from_str(&yaml3).unwrap();

        assert_eq!("pip", parsed_yaml3.specs[0].render());
    }

    #[test]
    fn read_full_recipe() {
        let test_data_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("test-data/rendered_recipes");
        let recipe_1 = test_data_dir.join("rich_recipe.yaml");

        let recipe_1 = std::fs::read_to_string(recipe_1).unwrap();

        let output_rich: Output = serde_yaml::from_str(&recipe_1).unwrap();
        assert_yaml_snapshot!(output_rich);

        let recipe_2 = test_data_dir.join("curl_recipe.yaml");
        let recipe_2 = std::fs::read_to_string(recipe_2).unwrap();
        let output_curl: Output = serde_yaml::from_str(&recipe_2).unwrap();
        assert_yaml_snapshot!(output_curl);
    }
}
