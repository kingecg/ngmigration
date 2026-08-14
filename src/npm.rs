//! Minimal npm registry client used to find the newest version of a package
//! whose `peerDependencies["@angular/core"]` range is satisfied by a target
//! Angular major.
//!
//! Compiled only with the `network` feature.

const REGISTRY: &str = "https://registry.npmjs.org";

/// Find the newest version of `package` whose Angular peer range accepts
/// Angular major `target`. Returns `None` when the package has no `@angular`
/// peer dependency, no compatible version is found, or the network fails.
pub fn find_compatible_version(package: &str, target: u32) -> Option<String> {
    #[cfg(feature = "network")]
    {
        let url = format!("{REGISTRY}/{}", package.replace('/', "%2F"));
        let resp = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .call()
            .ok()?;
        let json: serde_json::Value = resp.into_json().ok()?;

        // Gather stable versions, newest first.
        let mut versions: Vec<(u64, String)> = Vec::new();
        if let Some(vs) = json.get("versions").and_then(|v| v.as_object()) {
            for (v, meta) in vs {
                if v.contains('-') {
                    continue;
                }
                let dist = meta
                    .get("peerDependencies")
                    .and_then(|p| p.get("@angular/core"));
                if dist.is_none() {
                    continue; // not Angular-coupled
                }
                if let Ok(sem) = semver::Version::parse(v) {
                    versions.push((key(&sem), v.clone()));
                }
            }
        }
        versions.sort_by_key(|(k, _)| std::cmp::Reverse(*k));

        for (_, v) in versions.into_iter().take(40) {
            let Some(meta) = json["versions"].get(&v) else {
                continue;
            };
            let Some(range) = meta["peerDependencies"]["@angular/core"].as_str() else {
                continue;
            };
            let Ok(req) = semver::VersionReq::parse(range) else {
                continue;
            };
            if req.matches(&target_semver(target)) {
                return Some(format!("^{v}"));
            }
        }
        None
    }
    #[cfg(not(feature = "network"))]
    {
        let _ = (package, target);
        None
    }
}

fn key(v: &semver::Version) -> u64 {
    v.major << 32 | v.minor << 16 | v.patch
}

/// Verify a parsed Angular major can be expressed as a semver for range checks.
pub fn target_semver(target: u32) -> semver::Version {
    semver::Version::new(target as u64, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ordering() {
        let a = semver::Version::new(1, 9, 0);
        let b = semver::Version::new(1, 10, 0);
        assert!(key(&b) > key(&a));
    }
}
