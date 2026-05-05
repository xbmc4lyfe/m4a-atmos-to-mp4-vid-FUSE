use std::collections::{BTreeMap, HashMap};
#[cfg(not(test))]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(test))]
use std::time::SystemTime;

use anyhow::{Context, Result};
#[cfg(not(test))]
use fuser::{
    Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags, Generation, INodeNo,
    OpenAccMode, OpenFlags, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry, ReplyOpen, Request,
};
#[cfg(not(test))]
use std::time::Duration;

use crate::media::MediaItem;

pub trait CacheProvider: Send + Sync {
    fn ensure_cached(&self, item: &MediaItem) -> Result<PathBuf>;
    fn cached_path_if_exists(&self, item: &MediaItem) -> Option<PathBuf>;
}

#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub ino: u64,
    pub parent: u64,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, allow(dead_code))]
enum NodeKind {
    Directory { children: BTreeMap<OsString, u64> },
    File { item_index: usize },
}

#[derive(Debug, Clone)]
#[cfg_attr(test, allow(dead_code))]
struct Node {
    ino: u64,
    parent: u64,
    name: OsString,
    path: PathBuf,
    kind: NodeKind,
}

#[cfg_attr(test, allow(dead_code))]
pub struct AtmosFs {
    items: Vec<MediaItem>,
    cache: Arc<dyn CacheProvider>,
    nodes: HashMap<u64, Node>,
    paths: HashMap<PathBuf, u64>,
}

impl AtmosFs {
    pub fn new(items: Vec<MediaItem>, cache: Arc<dyn CacheProvider>) -> Self {
        let mut nodes = HashMap::new();
        let mut paths = HashMap::new();
        nodes.insert(
            1,
            Node {
                ino: 1,
                parent: 1,
                name: OsString::new(),
                path: PathBuf::new(),
                kind: NodeKind::Directory {
                    children: BTreeMap::new(),
                },
            },
        );
        paths.insert(PathBuf::new(), 1);
        let mut next_ino = 2;

        for (item_index, item) in items.iter().enumerate() {
            let mut parent = 1;
            let mut current_path = PathBuf::new();
            let components = item.virtual_path.iter().collect::<Vec<_>>();

            for component in &components[..components.len().saturating_sub(1)] {
                current_path.push(component);
                if let Some(ino) = paths.get(&current_path) {
                    parent = *ino;
                    continue;
                }

                let ino = next_ino;
                next_ino += 1;
                nodes.insert(
                    ino,
                    Node {
                        ino,
                        parent,
                        name: component.to_os_string(),
                        path: current_path.clone(),
                        kind: NodeKind::Directory {
                            children: BTreeMap::new(),
                        },
                    },
                );
                insert_child(&mut nodes, parent, component.to_os_string(), ino);
                paths.insert(current_path.clone(), ino);
                parent = ino;
            }

            if let Some(file_name) = components.last() {
                let mut file_path = current_path;
                file_path.push(file_name);
                if paths.contains_key(&file_path) {
                    log::warn!("skipping duplicate virtual path {}", file_path.display());
                    continue;
                }
                let ino = next_ino;
                next_ino += 1;
                nodes.insert(
                    ino,
                    Node {
                        ino,
                        parent,
                        name: file_name.to_os_string(),
                        path: file_path.clone(),
                        kind: NodeKind::File { item_index },
                    },
                );
                insert_child(&mut nodes, parent, file_name.to_os_string(), ino);
                paths.insert(file_path, ino);
            }
        }

        Self {
            items,
            cache,
            nodes,
            paths,
        }
    }

    pub fn node_by_path(&self, path: &str) -> Option<NodeInfo> {
        let path = PathBuf::from(path);
        self.paths
            .get(&path)
            .and_then(|ino| self.nodes.get(ino))
            .map(NodeInfo::from)
    }

    pub fn read_cached_slice(path: &Path, offset: u64, size: u32) -> Result<Vec<u8>> {
        let mut file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        file.seek(SeekFrom::Start(offset))
            .with_context(|| format!("failed to seek {}", path.display()))?;
        let mut data = Vec::new();
        file.take(u64::from(size))
            .read_to_end(&mut data)
            .with_context(|| format!("failed to read {}", path.display()))?;
        Ok(data)
    }

    fn materialized_size_for_item(&self, item: &MediaItem) -> Result<u64> {
        let cache_path = self
            .cache
            .cached_path_if_exists(item)
            .map(Ok)
            .unwrap_or_else(|| self.cache.ensure_cached(item))?;
        Ok(std::fs::metadata(&cache_path)
            .with_context(|| format!("failed to stat {}", cache_path.display()))?
            .len())
    }

    pub fn nodes(&self) -> Vec<NodeInfo> {
        let mut nodes = self.nodes.values().map(NodeInfo::from).collect::<Vec<_>>();
        nodes.sort_by_key(|node| node.ino);
        nodes
    }

    #[cfg(not(test))]
    fn lookup_child(&self, parent: u64, name: &OsStr) -> Option<&Node> {
        let parent = self.nodes.get(&parent)?;
        let NodeKind::Directory { children } = &parent.kind else {
            return None;
        };
        children.get(name).and_then(|ino| self.nodes.get(ino))
    }

    #[cfg(not(test))]
    fn attr_for_node(&self, node: &Node) -> Result<FileAttr> {
        match node.kind {
            NodeKind::Directory { .. } => Ok(file_attr(
                node.ino,
                0,
                FileType::Directory,
                0o555,
                current_time(),
            )),
            NodeKind::File { item_index } => {
                let item = &self.items[item_index];
                Ok(file_attr(
                    node.ino,
                    self.materialized_size_for_item(item)?,
                    FileType::RegularFile,
                    0o444,
                    item.mtime,
                ))
            }
        }
    }

    #[cfg(not(test))]
    fn cached_path_for_node(&self, ino: u64) -> Result<PathBuf> {
        let node = self.nodes.get(&ino).context("inode not found")?;
        let NodeKind::File { item_index } = node.kind else {
            anyhow::bail!("inode is not a file");
        };
        self.cache.ensure_cached(&self.items[item_index])
    }
}

impl NodeInfo {
    fn from(node: &Node) -> Self {
        Self {
            ino: node.ino,
            parent: node.parent,
            name: node.name.to_string_lossy().to_string(),
            is_dir: matches!(node.kind, NodeKind::Directory { .. }),
        }
    }
}

#[cfg(not(test))]
impl Filesystem for AtmosFs {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let Some(node) = self.lookup_child(parent.0, name) else {
            reply.error(Errno::ENOENT);
            return;
        };
        match self.attr_for_node(node) {
            Ok(attr) => reply.entry(&ttl(), &attr, Generation(0)),
            Err(error) => {
                log::error!("lookup failed for {:?}: {error:#}", node.path);
                reply.error(Errno::EIO);
            }
        }
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let Some(node) = self.nodes.get(&ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        match self.attr_for_node(node) {
            Ok(attr) => reply.attr(&ttl(), &attr),
            Err(error) => {
                log::error!("getattr failed for {:?}: {error:#}", node.path);
                reply.error(Errno::EIO);
            }
        }
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let Some(node) = self.nodes.get(&ino.0) else {
            reply.error(Errno::ENOENT);
            return;
        };
        let NodeKind::Directory { children } = &node.kind else {
            reply.error(Errno::ENOTDIR);
            return;
        };

        let mut entries = vec![
            (node.ino, FileType::Directory, OsString::from(".")),
            (node.parent, FileType::Directory, OsString::from("..")),
        ];
        entries.extend(children.iter().filter_map(|(name, child_ino)| {
            let child = self.nodes.get(child_ino)?;
            Some((
                child.ino,
                match child.kind {
                    NodeKind::Directory { .. } => FileType::Directory,
                    NodeKind::File { .. } => FileType::RegularFile,
                },
                name.clone(),
            ))
        }));

        for (entry_offset, (entry_ino, kind, name)) in
            entries.into_iter().enumerate().skip(offset as usize)
        {
            if reply.add(INodeNo(entry_ino), (entry_offset + 1) as u64, kind, name) {
                break;
            }
        }
        reply.ok();
    }

    fn open(&self, _req: &Request, ino: INodeNo, flags: OpenFlags, reply: ReplyOpen) {
        if flags.acc_mode() != OpenAccMode::O_RDONLY {
            reply.error(Errno::EACCES);
            return;
        }
        match self.cached_path_for_node(ino.0) {
            Ok(_) => reply.opened(FileHandle(0), FopenFlags::empty()),
            Err(error) => {
                log::error!("open failed for inode {ino}: {error:#}");
                reply.error(Errno::EIO);
            }
        }
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<fuser::LockOwner>,
        reply: ReplyData,
    ) {
        match self
            .cached_path_for_node(ino.0)
            .and_then(|path| Self::read_cached_slice(&path, offset, size))
        {
            Ok(data) => reply.data(&data),
            Err(error) => {
                log::error!("read failed for inode {ino}: {error:#}");
                reply.error(Errno::EIO);
            }
        }
    }
}

fn insert_child(nodes: &mut HashMap<u64, Node>, parent: u64, name: OsString, ino: u64) {
    if let Some(Node {
        kind: NodeKind::Directory { children },
        ..
    }) = nodes.get_mut(&parent)
    {
        children.insert(name, ino);
    }
}

#[cfg(not(test))]
fn current_time() -> SystemTime {
    SystemTime::now()
}

#[cfg(not(test))]
fn ttl() -> Duration {
    Duration::from_secs(1)
}

#[cfg(not(test))]
fn file_attr(ino: u64, size: u64, kind: FileType, perm: u16, mtime: SystemTime) -> FileAttr {
    FileAttr {
        ino: INodeNo(ino),
        size,
        blocks: size.div_ceil(512),
        atime: current_time(),
        mtime,
        ctime: mtime,
        crtime: mtime,
        kind,
        perm,
        nlink: if kind == FileType::Directory { 2 } else { 1 },
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
        rdev: 0,
        blksize: 4096,
        flags: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    struct NullCache;

    impl CacheProvider for NullCache {
        fn ensure_cached(&self, _item: &MediaItem) -> Result<PathBuf> {
            unreachable!("tree construction should not materialize cache")
        }

        fn cached_path_if_exists(&self, _item: &MediaItem) -> Option<PathBuf> {
            None
        }
    }

    struct MaterializingCache {
        path: PathBuf,
    }

    impl CacheProvider for MaterializingCache {
        fn ensure_cached(&self, _item: &MediaItem) -> Result<PathBuf> {
            Ok(self.path.clone())
        }

        fn cached_path_if_exists(&self, _item: &MediaItem) -> Option<PathBuf> {
            None
        }
    }

    fn media(path: &str, virtual_path: &str) -> MediaItem {
        MediaItem {
            source_path: PathBuf::from(path),
            relative_m4a: PathBuf::from(path),
            virtual_path: PathBuf::from(virtual_path),
            size: 1,
            mtime: UNIX_EPOCH,
        }
    }

    #[test]
    fn inode_tree_contains_root_directories_and_files() {
        let fs = AtmosFs::new(
            vec![
                media("/src/album/song.m4a", "album/song.mkv"),
                media("/src/album/disc2/other.m4a", "album/disc2/other.mkv"),
            ],
            std::sync::Arc::new(NullCache),
        );

        assert_eq!(fs.nodes()[0].ino, 1);
        assert!(fs.node_by_path("album").unwrap().is_dir);
        assert!(fs.node_by_path("album/disc2").unwrap().is_dir);
        assert!(!fs.node_by_path("album/song.mkv").unwrap().is_dir);
    }

    #[test]
    fn cached_read_slice_honors_offset_and_size() -> Result<()> {
        let tmp = tempfile::NamedTempFile::new()?;
        std::fs::write(tmp.path(), b"0123456789")?;

        assert_eq!(AtmosFs::read_cached_slice(tmp.path(), 2, 4)?, b"2345");
        assert_eq!(AtmosFs::read_cached_slice(tmp.path(), 8, 99)?, b"89");
        assert!(AtmosFs::read_cached_slice(tmp.path(), 99, 10)?.is_empty());
        Ok(())
    }

    #[test]
    fn materialized_size_uses_generated_mkv_size_when_cache_is_missing() -> Result<()> {
        let tmp = tempfile::NamedTempFile::new()?;
        std::fs::write(tmp.path(), b"generated mkv bytes")?;
        let fs = AtmosFs::new(
            vec![media("/src/album/song.m4a", "album/song.mkv")],
            std::sync::Arc::new(MaterializingCache {
                path: tmp.path().to_path_buf(),
            }),
        );

        let size = fs.materialized_size_for_item(&fs.items[0])?;

        assert_eq!(size, 19);
        Ok(())
    }
}
