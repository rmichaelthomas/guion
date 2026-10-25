use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub fn write_report(package_root: &Path) -> Result<String, String> {
    if cfg!(target_os = "linux") {
        return write_linux_report(package_root);
    }
    if !cfg!(target_os = "macos") {
        return Err("native dependency auditing supports macOS and Linux targets".into());
    }
    let root = fs::canonicalize(package_root)
        .map_err(|error| format!("cannot resolve package root for native audit: {error}"))?;
    let python_owners = scan_python_owners(&root)?;
    let cached_python_owners = scan_cached_python_owners(&root)?;
    let python = find_python_executable(&root)?;
    let seeds = find_native_images(&root, python.as_deref())?;
    let mut queue = VecDeque::new();
    for path in seeds {
        let executable = if python.is_some()
            && (path.starts_with(root.join("python"))
                || path.starts_with(root.join(".guion/python-bridge")))
        {
            python.clone().unwrap()
        } else {
            path.clone()
        };
        queue.push_back((path, executable, Vec::<PathBuf>::new()));
    }

    let mut scanned = BTreeMap::<PathBuf, serde_json::Value>::new();
    let mut bundled_targets = BTreeSet::new();
    let mut system_targets = BTreeSet::new();
    let mut visited_context = BTreeSet::<(PathBuf, PathBuf)>::new();
    while let Some((image, executable, inherited_rpaths)) = queue.pop_front() {
        let image = fs::canonicalize(&image)
            .map_err(|error| format!("cannot resolve native image {}: {error}", image.display()))?;
        if !image.starts_with(&root) {
            return Err(format!(
                "native image escapes package root: {}",
                image.display()
            ));
        }
        if !visited_context.insert((image.clone(), executable.clone())) {
            continue;
        }
        let load_output = run_otool(&["-L"], &image)?;
        let id_output = run_otool(&["-D"], &image)?;
        let ids = parse_otool_ids(&id_output);
        let dependencies = parse_otool_loads(&load_output)
            .into_iter()
            .filter(|dependency| !ids.contains(dependency))
            .collect::<BTreeSet<_>>();
        let own_rpaths = parse_otool_rpaths(&run_otool(&["-l"], &image)?);
        let mut all_rpaths = own_rpaths.clone();
        all_rpaths.extend(inherited_rpaths.iter().cloned());
        if image != executable {
            let executable_rpaths = parse_otool_rpaths(&run_otool(&["-l"], &executable)?);
            all_rpaths.extend(executable_rpaths);
        }
        let mut dependency_records = Vec::new();
        for load_name in dependencies {
            match resolve_load_name(&root, &image, &executable, &load_name, &all_rpaths)? {
                ResolvedLoad::System(path) => {
                    system_targets.insert(path.clone());
                    dependency_records.push(serde_json::json!({"load_name": load_name, "kind": "system", "resolved_path": path}));
                }
                ResolvedLoad::Bundled(path) => {
                    let relative = path.strip_prefix(&root).unwrap().display().to_string();
                    bundled_targets.insert(relative.clone());
                    dependency_records.push(serde_json::json!({"load_name": load_name, "kind": "bundled", "resolved_path": relative}));
                    queue.push_back((path, executable.clone(), all_rpaths.clone()));
                }
            }
        }
        let relative = image.strip_prefix(&root).unwrap().display().to_string();
        if scanned.contains_key(&image) {
            continue;
        }
        let (sha256, size_bytes) = hash_file(&image)?;
        let owner = owner_for_image(&root, &image, &python_owners)?;
        let architectures = run_lipo(&image)?;
        let expected_architecture = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x86_64",
            other => other,
        };
        if !architectures
            .iter()
            .any(|architecture| architecture == expected_architecture)
        {
            return Err(format!(
                "native image {} does not contain target architecture {expected_architecture}",
                image.display()
            ));
        }
        scanned.insert(
            image.clone(),
            serde_json::json!({
                "path": relative,
                "sha256": sha256,
                "size_bytes": size_bytes,
                "architectures": architectures,
                "dependencies": dependency_records,
                "owner": owner,
            }),
        );
    }
    let images = scanned.into_values().collect::<Vec<_>>();
    let cached_images = scan_cached_native_images(&root, &cached_python_owners)?;
    let unattributed_images = images
        .iter()
        .filter(|image| image["owner"]["kind"] == "unattributed")
        .filter_map(|image| image["path"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let bundled_targets = bundled_targets.into_iter().collect::<Vec<_>>();
    let system_targets = system_targets.into_iter().collect::<Vec<_>>();
    let report = serde_json::json!({
        "schema_version": 1,
        "target": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        "scanner": "macOS otool -L/-D/-l plus lipo -archs",
        "scope": "app runtime images, managed Python, installed Python extensions, declared resources, and native files in extracted offline uv wheel-cache distributions",
        "images": images,
        "cached_images": cached_images,
        "unattributed_images": unattributed_images,
        "bundled_libraries": bundled_targets,
        "system_libraries": system_targets,
        "unresolved": [],
    });
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("cannot encode native dependency report: {error}"))?;
    let path = root.join("NATIVE-DEPENDENCIES.json");
    fs::write(&path, &bytes)
        .map_err(|error| format!("cannot write native dependency report: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn write_linux_report(package_root: &Path) -> Result<String, String> {
    if std::env::consts::ARCH != "aarch64" {
        return Err(format!(
            "Linux native dependency reporting currently supports aarch64, not {}",
            std::env::consts::ARCH
        ));
    }
    let root = fs::canonicalize(package_root)
        .map_err(|error| format!("cannot resolve package root for native audit: {error}"))?;
    let python_owners = scan_python_owners(&root)?;
    let cached_python_owners = scan_cached_python_owners(&root)?;
    let python = find_python_executable(&root)?;
    let seeds = find_native_images(&root, python.as_deref())?;
    let mut images = Vec::new();
    let mut bundled_targets = BTreeSet::new();
    let mut system_targets = BTreeSet::new();
    for path in seeds {
        let path = fs::canonicalize(&path)
            .map_err(|error| format!("cannot resolve native image {}: {error}", path.display()))?;
        if !path.starts_with(&root) {
            return Err(format!(
                "native image escapes package root: {}",
                path.display()
            ));
        }
        let owner = owner_for_image(&root, &path, &python_owners)?;
        let (record, bundled, system) = inspect_elf_image(&root, &path, owner)?;
        bundled_targets.extend(bundled);
        system_targets.extend(system);
        images.push(record);
    }
    images.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    let cached_images = scan_cached_elf_images(&root, &cached_python_owners)?;
    let unattributed_images = images
        .iter()
        .filter(|image| image["owner"]["kind"] == "unattributed")
        .filter_map(|image| image["path"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let report = serde_json::json!({
        "schema_version": 1,
        "target": "linux-aarch64-gnu",
        "scanner": "ELF readelf dynamic metadata and ldd dependency resolution",
        "scope": "app runtime images, managed Python, installed Python extensions, declared resources, and native files in extracted offline uv wheel-cache distributions",
        "images": images,
        "cached_images": cached_images,
        "unattributed_images": unattributed_images,
        "bundled_libraries": bundled_targets.into_iter().collect::<Vec<_>>(),
        "system_libraries": system_targets.into_iter().collect::<Vec<_>>(),
        "unresolved": [],
    });
    let bytes = serde_json::to_vec_pretty(&report)
        .map_err(|error| format!("cannot encode native dependency report: {error}"))?;
    fs::write(root.join("NATIVE-DEPENDENCIES.json"), &bytes)
        .map_err(|error| format!("cannot write native dependency report: {error}"))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn inspect_elf_image(
    root: &Path,
    image: &Path,
    owner: serde_json::Value,
) -> Result<(serde_json::Value, Vec<String>, Vec<String>), String> {
    let dynamic = run_readelf(&["-dW"], image)?;
    let architecture = parse_readelf_architecture(&run_readelf(&["-h"], image)?)?;
    if architecture != "AArch64" {
        return Err(format!(
            "native image {} has architecture {architecture}, expected AArch64",
            image.display()
        ));
    }
    let version_output = run_readelf(&["--version-info", "--wide"], image)?;
    let versions = parse_readelf_versions(&version_output);
    let resolutions = parse_ldd_resolutions(&run_ldd(image)?);
    let needed = parse_readelf_needed(&dynamic);
    let mut dependencies = Vec::new();
    let mut bundled_targets = Vec::new();
    let mut system_targets = Vec::new();
    for load_name in needed {
        let expanded_origin = expand_elf_origin(&load_name, image);
        let resolved = if expanded_origin.is_none() {
            Some(resolutions.get(&load_name).ok_or_else(|| {
                format!(
                    "ldd did not resolve `{load_name}` required by {}",
                    image.display()
                )
            })?)
        } else {
            None
        };
        if resolved.is_some_and(|value| value.as_str() == "not found") {
            return Err(format!(
                "unresolved native dependency `{load_name}` required by {}",
                image.display()
            ));
        }
        let (resolved_path, dependency_kind) = if let Some(path) = expanded_origin {
            let resolved_path = fs::canonicalize(&path).map_err(|error| {
                format!(
                    "cannot resolve origin-relative dependency {} for {}: {error}",
                    path.display(),
                    image.display()
                )
            })?;
            let relative = resolved_path.starts_with(root);
            if relative {
                let package_path = resolved_path
                    .strip_prefix(root)
                    .unwrap()
                    .display()
                    .to_string();
                bundled_targets.push(package_path.clone());
                (package_path, "bundled")
            } else {
                let system_path = resolved_path.display().to_string();
                system_targets.push(system_path.clone());
                (system_path, "system")
            }
        } else if let Some(resolved) = resolved {
            if resolved == "linux-vdso.so.1" {
                system_targets.push(resolved.clone());
                (resolved.clone(), "system")
            } else {
                let resolved_path = fs::canonicalize(resolved).map_err(|error| {
                    format!(
                        "cannot resolve native dependency {resolved} for {}: {error}",
                        image.display()
                    )
                })?;
                if resolved_path.starts_with(root) {
                    let relative = resolved_path
                        .strip_prefix(root)
                        .unwrap()
                        .display()
                        .to_string();
                    bundled_targets.push(relative.clone());
                    (relative, "bundled")
                } else {
                    let system_path = resolved_path.display().to_string();
                    system_targets.push(system_path.clone());
                    (system_path, "system")
                }
            }
        } else {
            unreachable!("an origin-relative dependency is handled before ldd lookup")
        };
        dependencies.push(serde_json::json!({
            "load_name": load_name,
            "kind": dependency_kind,
            "resolved_path": resolved_path,
        }));
    }
    let (sha256, size_bytes) = hash_file(image)?;
    let relative = image.strip_prefix(root).unwrap().display().to_string();
    Ok((
        serde_json::json!({
            "path": relative,
            "sha256": sha256,
            "size_bytes": size_bytes,
            "architectures": ["aarch64"],
            "dependencies": dependencies,
            "required_glibc": versions.last(),
            "owner": owner,
        }),
        bundled_targets,
        system_targets,
    ))
}

fn scan_cached_elf_images(
    root: &Path,
    owners: &BTreeMap<PathBuf, Vec<serde_json::Value>>,
) -> Result<Vec<serde_json::Value>, String> {
    fn walk(path: &Path, images: &mut Vec<PathBuf>) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot scan cached native images: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot scan cached native images: {error}"))?;
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect cached native image: {error}"))?;
            if kind.is_dir() {
                walk(&entry.path(), images)?;
            } else if kind.is_file() && is_elf_file(&entry.path())? {
                images.push(entry.path());
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    walk(&root.join(".uv-cache"), &mut paths)?;
    paths.sort();
    let mut images = Vec::new();
    for path in paths {
        let path = fs::canonicalize(path)
            .map_err(|error| format!("cannot resolve cached native image: {error}"))?;
        let owner = owners.get(&path).ok_or_else(|| {
            format!(
                "cached native image has no owning Python distribution RECORD entry: {}",
                path.display()
            )
        })?;
        if owner.len() != 1 {
            return Err(format!(
                "cached native image {} is attributed to {} Python distributions",
                path.display(),
                owner.len()
            ));
        }
        let (mut record, _, _) = inspect_elf_image(root, &path, owner[0].clone())?;
        record["load_commands"] = record["dependencies"].clone();
        record.as_object_mut().unwrap().remove("dependencies");
        images.push(record);
    }
    Ok(images)
}

fn scan_cached_python_owners(
    root: &Path,
) -> Result<BTreeMap<PathBuf, Vec<serde_json::Value>>, String> {
    fn find_dist_info(path: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot inspect cached Python distributions: {error}"))?
        {
            let entry = entry.map_err(|error| {
                format!("cannot inspect cached Python distribution entry: {error}")
            })?;
            let kind = entry.file_type().map_err(|error| {
                format!("cannot inspect cached Python distribution entry: {error}")
            })?;
            if !kind.is_dir() {
                continue;
            }
            if entry.file_name().to_string_lossy().ends_with(".dist-info") {
                found.push(entry.path());
            } else {
                find_dist_info(&entry.path(), found)?;
            }
        }
        Ok(())
    }

    let cache = root.join(".uv-cache");
    let mut dist_infos = Vec::new();
    find_dist_info(&cache, &mut dist_infos)?;
    let mut owners = BTreeMap::<PathBuf, Vec<serde_json::Value>>::new();
    for dist_info in dist_infos {
        let metadata = fs::read_to_string(dist_info.join("METADATA")).map_err(|error| {
            format!(
                "cannot read cached Python distribution metadata {}: {error}",
                dist_info.display()
            )
        })?;
        let name = metadata_header(&metadata, "Name").ok_or_else(|| {
            format!(
                "cached Python distribution {} has no Name",
                dist_info.display()
            )
        })?;
        let version = metadata_header(&metadata, "Version")
            .ok_or_else(|| format!("cached Python distribution {name} has no Version"))?;
        let record = dist_info.join("RECORD");
        if !record.is_file() {
            continue;
        }
        let record_text = fs::read_to_string(&record)
            .map_err(|error| format!("cannot read {}: {error}", record.display()))?;
        let owner = serde_json::json!({
            "kind": "python_distribution",
            "name": name,
            "version": version,
            "environment": "offline-wheel-cache",
            "record": record.strip_prefix(root).unwrap().to_string_lossy(),
        });
        for row in record_text.lines() {
            let Some(relative) = parse_record_path(row) else {
                continue;
            };
            let Ok(path) = fs::canonicalize(dist_info.parent().unwrap().join(relative)) else {
                continue;
            };
            if path.starts_with(&cache) && path.is_file() {
                owners.entry(path).or_default().push(owner.clone());
            }
        }
    }
    Ok(owners)
}

fn scan_cached_native_images(
    root: &Path,
    owners: &BTreeMap<PathBuf, Vec<serde_json::Value>>,
) -> Result<Vec<serde_json::Value>, String> {
    fn walk(path: &Path, images: &mut Vec<PathBuf>) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot scan cached native images: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot scan cached native images: {error}"))?;
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect cached native image: {error}"))?;
            if kind.is_dir() {
                walk(&entry.path(), images)?;
            } else if kind.is_file() && is_macho_file(&entry.path())? {
                images.push(entry.path());
            }
        }
        Ok(())
    }

    let mut paths = Vec::new();
    walk(&root.join(".uv-cache"), &mut paths)?;
    paths.sort();
    let mut images = Vec::new();
    for path in paths {
        let path = fs::canonicalize(path)
            .map_err(|error| format!("cannot resolve cached native image: {error}"))?;
        let owner = owners.get(&path).ok_or_else(|| {
            format!(
                "cached native image has no owning Python distribution RECORD entry: {}",
                path.display()
            )
        })?;
        if owner.len() != 1 {
            return Err(format!(
                "cached native image {} is attributed to {} Python distributions",
                path.display(),
                owner.len()
            ));
        }
        let load_output = run_otool(&["-L"], &path)?;
        let ids = parse_otool_ids(&run_otool(&["-D"], &path)?);
        let load_commands = parse_otool_loads(&load_output)
            .into_iter()
            .filter(|load| !ids.contains(load))
            .collect::<Vec<_>>();
        let architectures = run_lipo(&path)?;
        let expected_architecture = match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x86_64",
            other => other,
        };
        if !architectures
            .iter()
            .any(|architecture| architecture == expected_architecture)
        {
            return Err(format!(
                "cached native image {} does not contain target architecture {expected_architecture}",
                path.display()
            ));
        }
        let (sha256, size_bytes) = hash_file(&path)?;
        images.push(serde_json::json!({
            "path": path.strip_prefix(root).unwrap().display().to_string(),
            "sha256": sha256,
            "size_bytes": size_bytes,
            "architectures": architectures,
            "load_commands": load_commands,
            "owner": owner[0],
        }));
    }
    Ok(images)
}

fn scan_python_owners(root: &Path) -> Result<BTreeMap<PathBuf, Vec<serde_json::Value>>, String> {
    fn find_dist_info(path: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot inspect Python distributions: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("cannot inspect Python distribution: {error}"))?;
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect Python distribution entry: {error}"))?;
            if kind.is_dir() {
                if entry.file_name() == ".uv-cache" {
                    continue;
                }
                if entry.file_name().to_string_lossy().ends_with(".dist-info") {
                    found.push(entry.path());
                } else {
                    find_dist_info(&entry.path(), found)?;
                }
            }
        }
        Ok(())
    }

    let report_path = root.join("PYTHON-DEPENDENCIES.json");
    if !report_path.is_file() {
        return Ok(BTreeMap::new());
    }
    let report: serde_json::Value = serde_json::from_slice(
        &fs::read(&report_path)
            .map_err(|error| format!("cannot read Python dependency report: {error}"))?,
    )
    .map_err(|error| format!("invalid Python dependency report: {error}"))?;
    let packages = report["packages"]
        .as_array()
        .ok_or("invalid Python dependency report: packages must be an array")?;
    let mut dist_infos = Vec::new();
    find_dist_info(root, &mut dist_infos)?;
    let mut owners = BTreeMap::<PathBuf, Vec<serde_json::Value>>::new();
    for dist_info in dist_infos {
        let metadata = fs::read_to_string(dist_info.join("METADATA")).map_err(|error| {
            format!(
                "cannot read Python distribution metadata {}: {error}",
                dist_info.display()
            )
        })?;
        let name = metadata_header(&metadata, "Name")
            .ok_or_else(|| format!("Python distribution {} has no Name", dist_info.display()))?;
        let version = metadata_header(&metadata, "Version")
            .ok_or_else(|| format!("Python distribution {name} has no Version"))?;
        let environment = if dist_info.starts_with(root.join(".guion/python-bridge/.venv")) {
            "project-environment"
        } else if dist_info.starts_with(root.join("python")) {
            "managed-interpreter"
        } else {
            continue;
        };
        let Some(package) = packages.iter().find(|package| {
            package["environment"].as_str() == Some(environment)
                && package["name"].as_str().is_some_and(|value| {
                    normalize_distribution_name(value) == normalize_distribution_name(&name)
                })
                && package["version"].as_str() == Some(version.as_str())
        }) else {
            return Err(format!(
                "Python distribution {name} {version} is missing from dependency provenance"
            ));
        };
        let record = dist_info.join("RECORD");
        if !record.is_file() {
            continue;
        }
        let record_text = fs::read_to_string(&record)
            .map_err(|error| format!("cannot read {}: {error}", record.display()))?;
        let mut owned_images = BTreeSet::new();
        for row in record_text.lines() {
            let Some(relative) = parse_record_path(row) else {
                continue;
            };
            let Ok(path) = fs::canonicalize(dist_info.parent().unwrap().join(relative)) else {
                continue;
            };
            if path.starts_with(root) && path.is_file() {
                owned_images.insert(path);
            }
        }
        let license_files = package["license_files"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let owner = serde_json::json!({
            "kind": "python_distribution",
            "name": name,
            "version": version,
            "environment": environment,
            "record": record.strip_prefix(root).unwrap().to_string_lossy(),
            "license_status": package["license_status"],
            "license_files": license_files,
        });
        for image in owned_images {
            owners.entry(image).or_default().push(owner.clone());
        }
    }
    Ok(owners)
}

fn metadata_header(metadata: &str, key: &str) -> Option<String> {
    metadata.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(key)
            .then(|| value.trim().to_owned())
    })
}

fn normalize_distribution_name(name: &str) -> String {
    name.to_ascii_lowercase().replace(['_', '.'], "-")
}

fn parse_record_path(row: &str) -> Option<String> {
    let mut chars = row.chars();
    let mut field = String::new();
    if chars.next()? == '"' {
        loop {
            match chars.next()? {
                '"' if chars.clone().next() == Some('"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => return matches!(chars.next(), None | Some(',')).then_some(field),
                value => field.push(value),
            }
        }
    }
    let mut field = String::new();
    field.push(row.chars().next()?);
    for value in chars {
        if value == ',' {
            break;
        }
        field.push(value);
    }
    Some(field)
}

fn owner_for_image(
    root: &Path,
    image: &Path,
    python_owners: &BTreeMap<PathBuf, Vec<serde_json::Value>>,
) -> Result<serde_json::Value, String> {
    if let Some(owners) = python_owners.get(image) {
        if owners.len() != 1 {
            return Err(format!(
                "native image {} is attributed to {} Python distributions",
                image.display(),
                owners.len()
            ));
        }
        return Ok(owners[0].clone());
    }
    let relative = image.strip_prefix(root).unwrap().to_string_lossy();
    if relative
        .split('/')
        .any(|component| component == "site-packages")
    {
        return Err(format!(
            "native Python image has no owning distribution RECORD entry: {relative}"
        ));
    }
    let kind = if image.starts_with(root.join("python")) {
        "managed_python_runtime"
    } else if image == root.join("toolchains/uv") {
        "bundled_toolchain"
    } else if image == root.join("guion") {
        "guion_runtime"
    } else {
        "unattributed"
    };
    let component = match kind {
        "managed_python_runtime" => "CPython",
        "bundled_toolchain" => "uv",
        "guion_runtime" => "Guion",
        _ => "unknown",
    };
    Ok(serde_json::json!({"kind": kind, "component": component}))
}

enum ResolvedLoad {
    System(String),
    Bundled(PathBuf),
}

fn find_python_executable(root: &Path) -> Result<Option<PathBuf>, String> {
    let python_root = root.join("python");
    if !python_root.exists() {
        return Ok(None);
    }
    fn walk(path: &Path, matches: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(path)
            .map_err(|error| format!("cannot inspect managed Python runtime: {error}"))?
        {
            let entry = entry.map_err(|error| format!("cannot inspect Python runtime: {error}"))?;
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect Python runtime entry: {error}"))?;
            if kind.is_dir() {
                walk(&entry.path(), matches)?;
            } else if kind.is_file()
                && entry.file_name().to_string_lossy().starts_with("python3.")
                && entry
                    .path()
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "bin")
            {
                matches.push(entry.path());
            }
        }
        Ok(())
    }
    let mut matches = Vec::new();
    walk(&python_root, &mut matches)?;
    matches.sort();
    matches
        .into_iter()
        .next()
        .map(Some)
        .ok_or_else(|| "managed CPython executable is missing from package".into())
}

fn find_native_images(root: &Path, python: Option<&Path>) -> Result<Vec<PathBuf>, String> {
    fn walk(
        root: &Path,
        path: &Path,
        python: Option<&Path>,
        images: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(path).map_err(|error| {
            format!(
                "cannot scan package native images in {}: {error}",
                path.display()
            )
        })? {
            let entry =
                entry.map_err(|error| format!("cannot scan package native images: {error}"))?;
            let kind = entry
                .file_type()
                .map_err(|error| format!("cannot inspect package entry: {error}"))?;
            let child = entry.path();
            if kind.is_dir() {
                if entry.file_name() == ".uv-cache" {
                    continue;
                }
                walk(root, &child, python, images)?;
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let extension = child.extension().and_then(|value| value.to_str());
            if matches!(extension, Some("so" | "dylib"))
                || python.is_some_and(|python| child == python)
                || child == root.join("guion")
                || child == root.join("toolchains/uv")
                || is_macho_file(&child)?
            {
                images.push(child);
            }
        }
        Ok(())
    }
    let mut images = Vec::new();
    walk(root, root, python, &mut images)?;
    images.sort();
    images.dedup();
    Ok(images)
}

fn is_macho_file(path: &Path) -> Result<bool, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("cannot inspect package file {}: {error}", path.display()))?;
    let mut magic = [0u8; 4];
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(is_macho_magic(magic)),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect package file {}: {error}",
            path.display()
        )),
    }
}

fn is_macho_magic(magic: [u8; 4]) -> bool {
    matches!(
        magic,
        [0xfe, 0xed, 0xfa, 0xce]
            | [0xce, 0xfa, 0xed, 0xfe]
            | [0xfe, 0xed, 0xfa, 0xcf]
            | [0xcf, 0xfa, 0xed, 0xfe]
            | [0xca, 0xfe, 0xba, 0xbe]
            | [0xbe, 0xba, 0xfe, 0xca]
            | [0xca, 0xfe, 0xba, 0xbf]
            | [0xbf, 0xba, 0xfe, 0xca]
    )
}

fn is_elf_file(path: &Path) -> Result<bool, String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("cannot inspect package file {}: {error}", path.display()))?;
    let mut magic = [0u8; 4];
    match file.read_exact(&mut magic) {
        Ok(()) => Ok(is_elf_magic(magic)),
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(error) => Err(format!(
            "cannot inspect package file {}: {error}",
            path.display()
        )),
    }
}

fn is_elf_magic(magic: [u8; 4]) -> bool {
    magic == [0x7f, b'E', b'L', b'F']
}

fn parse_readelf_needed(output: &str) -> Vec<String> {
    let mut dependencies = output
        .lines()
        .filter_map(|line| {
            let (_, value) = line.split_once("(NEEDED)")?;
            let start = value.find('[')? + 1;
            let end = value[start..].find(']')? + start;
            Some(value[start..end].to_owned())
        })
        .collect::<Vec<_>>();
    dependencies.sort();
    dependencies.dedup();
    dependencies
}

fn parse_readelf_versions(output: &str) -> Vec<String> {
    let mut versions = output
        .lines()
        .filter_map(|line| {
            let (_, value) = line.split_once("Name: GLIBC_")?;
            let version = value.split_whitespace().next()?;
            Some(version.to_owned())
        })
        .collect::<Vec<_>>();
    versions.sort_by(|left, right| compare_versions(left, right));
    versions.dedup();
    versions
}

fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    let parse = |value: &str| {
        value
            .split('.')
            .map(|part| part.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    parse(left).cmp(&parse(right))
}

fn parse_readelf_architecture(output: &str) -> Result<String, String> {
    output
        .lines()
        .find_map(|line| line.trim().strip_prefix("Machine:").map(str::trim))
        .map(str::to_owned)
        .ok_or_else(|| "readelf did not report an ELF machine architecture".to_owned())
}

fn expand_elf_origin(load_name: &str, image: &Path) -> Option<PathBuf> {
    let relative = load_name
        .strip_prefix("${ORIGIN}/")
        .or_else(|| load_name.strip_prefix("$ORIGIN/"))?;
    Some(image.parent()?.join(relative))
}

fn parse_ldd_resolutions(output: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some((name, target)) = line.split_once("=>") {
            let name = name.trim();
            let target = target.trim();
            let resolved = if target.starts_with("not found") {
                "not found".to_owned()
            } else {
                target
                    .rsplit_once(" (")
                    .map(|(path, _address)| path.trim())
                    .unwrap_or(target)
                    .to_owned()
            };
            result.insert(name.to_owned(), resolved);
        } else if let Some((path, _)) = line.split_once(" (") {
            let path = path.trim();
            let name = Path::new(path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(path);
            result.insert(name.to_owned(), path.to_owned());
        }
    }
    result
}

fn run_readelf(args: &[&str], image: &Path) -> Result<String, String> {
    let output = Command::new("readelf")
        .args(args)
        .arg(image)
        .output()
        .map_err(|error| format!("cannot run readelf for {}: {error}", image.display()))?;
    ensure_tool_success("readelf", output, image)
}

fn run_ldd(image: &Path) -> Result<String, String> {
    let output = Command::new("ldd")
        .arg(image)
        .output()
        .map_err(|error| format!("cannot run ldd for {}: {error}", image.display()))?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if text.contains("not found") || stderr.contains("not found") {
        let missing = text
            .lines()
            .chain(stderr.lines())
            .find(|line| line.contains("not found"))
            .unwrap_or("unresolved library");
        return Err(format!(
            "unresolved transitive ELF dependency for {}: {}",
            image.display(),
            missing.trim()
        ));
    }
    if !output.status.success() && !text.contains("not a dynamic executable") {
        return Err(format!(
            "ldd could not inspect {}: {}",
            image.display(),
            stderr.trim()
        ));
    }
    Ok(text)
}

fn run_otool(args: &[&str], image: &Path) -> Result<String, String> {
    let output = Command::new("/usr/bin/otool")
        .args(args)
        .arg(image)
        .output()
        .map_err(|error| format!("cannot run macOS otool for {}: {error}", image.display()))?;
    ensure_tool_success("otool", output, image)
}

fn run_lipo(image: &Path) -> Result<Vec<String>, String> {
    let output = Command::new("/usr/bin/lipo")
        .args(["-archs"])
        .arg(image)
        .output()
        .map_err(|error| format!("cannot run macOS lipo for {}: {error}", image.display()))?;
    let text = ensure_tool_success("lipo", output, image)?;
    let architectures = text
        .split_whitespace()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if architectures.is_empty() {
        return Err(format!(
            "lipo reported no architecture for {}",
            image.display()
        ));
    }
    Ok(architectures)
}

fn ensure_tool_success(tool: &str, output: Output, image: &Path) -> Result<String, String> {
    if !output.status.success() {
        return Err(format!(
            "{tool} could not inspect {}: {}",
            image.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| {
        format!(
            "{tool} output for {} is not UTF-8: {error}",
            image.display()
        )
    })
}

fn parse_otool_ids(output: &str) -> BTreeSet<String> {
    let mut lines = output.lines();
    let _header = lines.next();
    lines
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("(architecture "))
        .map(str::to_owned)
        .collect()
}

fn parse_otool_loads(output: &str) -> Vec<String> {
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            line.trim()
                .split_once(" (compatibility version")
                .map(|(name, _)| name.to_owned())
        })
        .collect()
}

fn parse_otool_rpaths(output: &str) -> Vec<PathBuf> {
    let lines = output.lines().collect::<Vec<_>>();
    let mut paths = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "cmd LC_RPATH" {
            if let Some(path_line) = lines.get(index + 2) {
                if let Some((_, path)) = path_line.trim().split_once("path ") {
                    paths.push(PathBuf::from(path.split(" (offset").next().unwrap_or(path)));
                }
            }
        }
    }
    paths
}

fn resolve_load_name(
    root: &Path,
    image: &Path,
    executable: &Path,
    load_name: &str,
    rpaths: &[PathBuf],
) -> Result<ResolvedLoad, String> {
    if load_name.starts_with("/usr/lib/") || load_name.starts_with("/System/Library/") {
        return Ok(ResolvedLoad::System(load_name.to_owned()));
    }
    let candidates = if let Some(rest) = load_name.strip_prefix("@loader_path/") {
        vec![image.parent().unwrap().join(rest)]
    } else if let Some(rest) = load_name.strip_prefix("@executable_path/") {
        vec![executable.parent().unwrap().join(rest)]
    } else if let Some(rest) = load_name.strip_prefix("@rpath/") {
        rpaths
            .iter()
            .map(|rpath| expand_rpath(rpath, image, executable).join(rest))
            .collect()
    } else if Path::new(load_name).is_absolute() {
        vec![PathBuf::from(load_name)]
    } else {
        let mut candidates = vec![image.parent().unwrap().join(load_name)];
        candidates.extend(
            rpaths
                .iter()
                .map(|rpath| expand_rpath(rpath, image, executable).join(load_name)),
        );
        candidates
    };
    let mut resolved = BTreeSet::new();
    for candidate in candidates {
        if let Ok(path) = fs::canonicalize(candidate) {
            if path.is_file() && path.starts_with(root) {
                resolved.insert(path);
            } else if path.is_file()
                && (path.starts_with("/usr/lib") || path.starts_with("/System/Library"))
            {
                return Ok(ResolvedLoad::System(path.display().to_string()));
            }
        }
    }
    match resolved.len() {
        1 => Ok(ResolvedLoad::Bundled(resolved.into_iter().next().unwrap())),
        0 => Err(format!(
            "unresolved native dependency `{load_name}` referenced by {}",
            image.display()
        )),
        _ => Err(format!(
            "ambiguous native dependency `{load_name}` referenced by {}",
            image.display()
        )),
    }
}

fn expand_rpath(rpath: &Path, image: &Path, executable: &Path) -> PathBuf {
    let value = rpath.to_string_lossy();
    if let Some(rest) = value.strip_prefix("@loader_path") {
        image.parent().unwrap().join(rest.trim_start_matches('/'))
    } else if let Some(rest) = value.strip_prefix("@executable_path") {
        executable
            .parent()
            .unwrap()
            .join(rest.trim_start_matches('/'))
    } else {
        rpath.to_path_buf()
    }
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = fs::File::open(path)
        .map_err(|error| format!("cannot read native image {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("cannot hash native image {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        size = size
            .checked_add(read as u64)
            .ok_or_else(|| format!("native image {} is too large", path.display()))?;
    }
    Ok((format!("{:x}", digest.finalize()), size))
}

#[cfg(test)]
mod tests {
    use super::{
        expand_elf_origin, is_elf_magic, is_macho_magic, parse_ldd_resolutions, parse_otool_ids,
        parse_otool_loads, parse_otool_rpaths, parse_readelf_needed, parse_readelf_versions,
        parse_record_path,
    };

    #[test]
    fn detects_macho_images_without_relying_on_filename_extensions() {
        assert!(is_macho_magic([0xcf, 0xfa, 0xed, 0xfe]));
        assert!(is_macho_magic([0xca, 0xfe, 0xba, 0xbe]));
        assert!(!is_macho_magic([0x50, 0x4b, 0x03, 0x04]));
    }

    #[test]
    fn distinguishes_macho_install_id_from_dynamic_loads() {
        let output = "sample.dylib:\n\t@rpath/sample.dylib (compatibility version 1.0.0, current version 1.0.0)\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n";
        let ids = parse_otool_ids("sample.dylib:\n@rpath/sample.dylib\n");
        let loads = parse_otool_loads(output);
        let actual = loads
            .into_iter()
            .filter(|load| !ids.contains(load))
            .collect::<Vec<_>>();
        assert_eq!(actual, ["/usr/lib/libSystem.B.dylib"]);
    }

    #[test]
    fn reads_each_runpath_from_load_commands() {
        let output = "Load command 4\n          cmd LC_RPATH\n      cmdsize 32\n         path @loader_path/../.. (offset 12)\nLoad command 5\n          cmd LC_RPATH\n      cmdsize 32\n         path /opt/vendor/lib (offset 12)\n";
        assert_eq!(
            parse_otool_rpaths(output),
            ["@loader_path/../..", "/opt/vendor/lib"].map(std::path::PathBuf::from)
        );
    }

    #[test]
    fn parses_quoted_python_record_paths_with_commas() {
        assert_eq!(
            parse_record_path("package/normal.so,sha256=abc,12"),
            Some("package/normal.so".into())
        );
        assert_eq!(
            parse_record_path("\"package/a,b.so\",sha256=abc,12"),
            Some("package/a,b.so".into())
        );
        assert_eq!(
            parse_record_path("\"package/a\"\"b.so\",sha256=abc,12"),
            Some("package/a\"b.so".into())
        );
    }

    #[test]
    fn detects_elf_images_without_relying_on_filename_extensions() {
        assert!(is_elf_magic([0x7f, b'E', b'L', b'F']));
        assert!(!is_elf_magic([0xcf, 0xfa, 0xed, 0xfe]));
    }

    #[test]
    fn parses_elf_direct_dependencies_and_runtime_resolutions() {
        let dynamic = " 0x0000000000000001 (NEEDED) Shared library: [libc.so.6]\n 0x0000000000000001 (NEEDED) Shared library: [libm.so.6]\n";
        assert_eq!(parse_readelf_needed(dynamic), ["libc.so.6", "libm.so.6"]);
        let ldd = "linux-vdso.so.1 (0x0000)\nlibc.so.6 => /lib/aarch64-linux-gnu/libc.so.6 (0x0001)\nlibm.so.6 => not found\n";
        let resolutions = parse_ldd_resolutions(ldd);
        assert_eq!(
            resolutions.get("libc.so.6").map(String::as_str),
            Some("/lib/aarch64-linux-gnu/libc.so.6")
        );
        assert_eq!(
            resolutions.get("libm.so.6").map(String::as_str),
            Some("not found")
        );
        assert_eq!(
            parse_readelf_versions(
                "  Name: GLIBC_2.17  Flags: none\n  Name: GLIBC_2.34  Flags: none\n"
            ),
            ["2.17", "2.34"]
        );
    }

    #[test]
    fn parses_linux_dependency_paths_that_contain_spaces() {
        let ldd = "libtk8.6.so => /tmp/guion package 起点 python run-160/python/lib/libtk8.6.so (0x0001)\n";
        let resolutions = parse_ldd_resolutions(ldd);
        assert_eq!(
            resolutions.get("libtk8.6.so").map(String::as_str),
            Some("/tmp/guion package 起点 python run-160/python/lib/libtk8.6.so")
        );
    }

    #[test]
    fn expands_origin_relative_elf_dependencies_from_the_image_directory() {
        let image = std::path::Path::new("/package/python/lib/libpython3.so");
        assert_eq!(
            expand_elf_origin("$ORIGIN/../lib/libpython3.14.so", image),
            Some(std::path::PathBuf::from(
                "/package/python/lib/../lib/libpython3.14.so"
            ))
        );
        assert_eq!(expand_elf_origin("libc.so.6", image), None);
    }
}
