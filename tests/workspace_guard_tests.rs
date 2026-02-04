use ouro::safety::workspace::WorkspaceGuard;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ─── Helper ───────────────────────────────────────────────────────────

fn setup_workspace() -> TempDir {
    tempfile::tempdir().expect("failed to create temp dir")
}

// ─── ALLOWED writes ──────────────────────────────────────────────────

#[test]
fn allows_write_to_file_directly_in_workspace() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    let target = tmp.path().join("file.txt");
    fs::write(&target, "data").unwrap();

    assert_eq!(guard.is_write_allowed(&target).unwrap(), true);
}

#[test]
fn allows_write_to_file_in_subdirectory() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    let subdir = tmp.path().join("sub").join("dir");
    fs::create_dir_all(&subdir).unwrap();
    let target = subdir.join("file.txt");
    fs::write(&target, "data").unwrap();

    assert_eq!(guard.is_write_allowed(&target).unwrap(), true);
}

#[test]
fn allows_write_to_new_file_parent_exists() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    // File does not exist yet, but parent (workspace root) does
    let target = tmp.path().join("new_file.txt");
    assert!(!target.exists());

    assert_eq!(guard.is_write_allowed(&target).unwrap(), true);
}

#[test]
fn allows_write_to_new_file_in_new_subdirectory() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    // Create the subdirectory first, then check a new file in it
    let subdir = tmp.path().join("new_sub");
    fs::create_dir_all(&subdir).unwrap();
    let target = subdir.join("file.txt");
    assert!(!target.exists());

    assert_eq!(guard.is_write_allowed(&target).unwrap(), true);
}

// ─── BLOCKED writes ─────────────────────────────────────────────────

#[test]
fn blocks_write_to_file_outside_workspace() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    let outside = std::env::temp_dir().join("outside_workspace_test.txt");
    fs::write(&outside, "data").unwrap();
    let result = guard.is_write_allowed(&outside).unwrap();
    // Clean up
    let _ = fs::remove_file(&outside);

    assert_eq!(result, false);
}

#[test]
fn blocks_path_traversal_via_dotdot() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    // ../../../etc/passwd -- resolves outside workspace
    let target = tmp.path().join("..").join("..").join("..").join("etc").join("passwd");

    assert_eq!(guard.is_write_allowed(&target).unwrap(), false);
}

#[test]
fn blocks_absolute_path_outside_workspace() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    let target = Path::new("/etc/hosts");
    assert_eq!(guard.is_write_allowed(target).unwrap(), false);
}

#[test]
fn blocks_write_to_home_directory() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    if let Some(home) = dirs_home() {
        let target = home.join(".bashrc");
        // .bashrc might not exist, but home dir does -> parent canonicalizable
        let result = guard.is_write_allowed(&target).unwrap();
        assert_eq!(result, false, "writes to home directory must be blocked");
    }
}

// ─── SYMLINK cases ──────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn blocks_symlink_pointing_outside_workspace() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    // Create a target outside workspace
    let outside_dir = tempfile::tempdir().expect("failed to create outside dir");
    let outside_file = outside_dir.path().join("target.txt");
    fs::write(&outside_file, "outside data").unwrap();

    // Create symlink inside workspace pointing to outside file
    let symlink_path = tmp.path().join("sneaky_link");
    std::os::unix::fs::symlink(&outside_file, &symlink_path).unwrap();

    // The symlink resolves outside workspace -- must be blocked
    assert_eq!(guard.is_write_allowed(&symlink_path).unwrap(), false);
}

#[cfg(unix)]
#[test]
fn allows_symlink_pointing_inside_workspace() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    // Create real file inside workspace
    let real_file = tmp.path().join("real.txt");
    fs::write(&real_file, "real data").unwrap();

    // Create symlink inside workspace pointing to real file (also inside)
    let symlink_path = tmp.path().join("internal_link");
    std::os::unix::fs::symlink(&real_file, &symlink_path).unwrap();

    // Resolves inside workspace -- should be allowed
    assert_eq!(guard.is_write_allowed(&symlink_path).unwrap(), true);
}

// ─── EDGE cases ─────────────────────────────────────────────────────

#[test]
fn allows_write_to_workspace_root_itself() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    // Writing to the workspace directory path itself
    assert_eq!(guard.is_write_allowed(tmp.path()).unwrap(), true);
}

#[test]
fn blocks_write_when_parent_does_not_exist() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    // Parent directory "no_such_parent" does not exist
    let target = tmp.path().join("no_such_parent").join("file.txt");
    assert!(!target.parent().unwrap().exists());

    assert_eq!(guard.is_write_allowed(&target).unwrap(), false);
}

#[test]
fn creates_workspace_directory_if_missing() {
    let tmp = setup_workspace();
    let new_ws = tmp.path().join("brand_new_workspace");
    assert!(!new_ws.exists());

    let guard = WorkspaceGuard::new(&new_ws).unwrap();
    assert!(new_ws.exists(), "workspace directory should have been created");

    // And it should be the canonical root
    let canonical = fs::canonicalize(&new_ws).unwrap();
    assert_eq!(guard.canonical_root(), canonical.as_path());
}

#[test]
fn canonical_root_returns_resolved_path() {
    let tmp = setup_workspace();
    let guard = WorkspaceGuard::new(tmp.path()).unwrap();

    let expected = fs::canonicalize(tmp.path()).unwrap();
    assert_eq!(guard.canonical_root(), expected.as_path());
}

// ─── Utility ────────────────────────────────────────────────────────

fn dirs_home() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(std::path::PathBuf::from)
}
