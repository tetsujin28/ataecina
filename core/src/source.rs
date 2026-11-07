//! Lecture d'une source (fichier image ou périphérique) avec des accès alignés,
//! obligatoires sur un périphérique Windows.

use std::fs::File;
use std::io;
use std::path::Path;

pub trait Source: Send + Sync {
    /// Lit jusqu'à `n` octets à `off` ; renvoie moins si la fin est atteinte ou si la lecture échoue.
    fn lire(&self, off: u64, n: usize) -> Vec<u8>;
    fn taille(&self) -> u64;
}

pub fn read_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.read_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        f.seek_read(buf, off)
    }
}

pub fn write_at(f: &File, buf: &[u8], off: u64) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        f.write_all_at(buf, off)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        let mut fait = 0;
        while fait < buf.len() {
            let n = f.seek_write(&buf[fait..], off + fait as u64)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::WriteZero, "write"));
            }
            fait += n;
        }
        Ok(())
    }
}

/// Lit exactement `buf.len()` octets si possible ; renvoie le nombre lu et l'erreur éventuelle.
pub fn read_full(f: &File, buf: &mut [u8], off: u64) -> (usize, Option<io::Error>) {
    let mut fait = 0;
    while fait < buf.len() {
        match read_at(f, &mut buf[fait..], off + fait as u64) {
            Ok(0) => return (fait, None),
            Ok(n) => fait += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return (fait, Some(e)),
        }
    }
    (fait, None)
}

pub struct FileSource {
    pub f: File,
    taille: u64,
    align: u64,
}

impl FileSource {
    pub fn ouvrir(p: &Path, peripherique: bool) -> io::Result<Self> {
        let f = File::open(p)?;
        let taille = crate::platform::taille_source(&f)?;
        if taille == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "taille"));
        }
        Ok(FileSource { f, taille, align: if peripherique { 4096 } else { 1 } })
    }
}

impl Source for FileSource {
    fn taille(&self) -> u64 {
        self.taille
    }
    fn lire(&self, off: u64, n: usize) -> Vec<u8> {
        if off >= self.taille || n == 0 {
            return vec![];
        }
        let n = n.min((self.taille - off) as usize);
        if self.align <= 1 {
            let mut b = vec![0u8; n];
            let (k, _) = read_full(&self.f, &mut b, off);
            b.truncate(k);
            return b;
        }
        let a = self.align;
        let debut = off / a * a;
        let fin = ((off + n as u64 + a - 1) / a * a).min((self.taille + a - 1) / a * a);
        let mut b = vec![0u8; (fin - debut) as usize];
        let (k, _) = read_full(&self.f, &mut b, debut);
        let d = (off - debut) as usize;
        if k <= d {
            return vec![];
        }
        b[d..k.min(d + n)].to_vec()
    }
}

/// Source en mémoire (tests).
pub struct MemSource(pub Vec<u8>);

impl Source for MemSource {
    fn taille(&self) -> u64 {
        self.0.len() as u64
    }
    fn lire(&self, off: u64, n: usize) -> Vec<u8> {
        let off = off as usize;
        if off >= self.0.len() {
            return vec![];
        }
        self.0[off..(off + n).min(self.0.len())].to_vec()
    }
}
