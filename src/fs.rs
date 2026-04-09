use std::io;
use std::path::{Path, PathBuf};

pub trait Filesystem {
    fn read_to_string(&self, path: &Path) -> io::Result<String>;
    fn write(&self, path: &Path, contents: &str) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;
}

// ── Real filesystem (production) ───────────────────────────────────────────

pub struct RealFilesystem;

impl Filesystem for RealFilesystem {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write(&self, path: &Path, contents: &str) -> io::Result<()> {
        std::fs::write(path, contents)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }
}

// ── Null filesystem (for tests) ────────────────────────────────────────────

pub struct NullFilesystem;

impl Filesystem for NullFilesystem {
    fn read_to_string(&self, _: &Path) -> io::Result<String> {
        Ok(String::new())
    }

    fn write(&self, _: &Path, _: &str) -> io::Result<()> {
        Ok(())
    }

    fn remove_file(&self, _: &Path) -> io::Result<()> {
        Ok(())
    }

    fn exists(&self, _: &Path) -> bool {
        false
    }

    fn create_dir_all(&self, _: &Path) -> io::Result<()> {
        Ok(())
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        Ok(path.to_path_buf())
    }
}
