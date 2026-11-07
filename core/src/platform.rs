//! Fonctions propres au système : lecteurs, taille des volumes, droits administrateur,
//! écriture de secteurs avec verrouillage, processus sans console.

use serde::Serialize;
use std::fs::File;
use std::io;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct Lecteur {
    pub letter: String,
    pub device: String,
    pub label: String,
    pub fs: String,
    pub size: u64,
    pub removable: bool,
    pub system: bool,
}

/// Empêche l'ouverture d'une console pour les outils lancés (ffmpeg, untrunc...).
pub fn sans_console(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c
}

/// PhotoRec a besoin d'une vraie console pour son affichage texte : on lui en donne une, réduite.
pub fn console_cachee(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    c
}

pub fn chemin_peripherique(drive: &str) -> String {
    #[cfg(windows)]
    {
        format!(r"\\.\{}", drive.trim_end_matches(['\\', '/']))
    }
    #[cfg(not(windows))]
    {
        drive.to_string()
    }
}

/// Lettre de lecteur d'un chemin ("E:"), en majuscules.
pub fn volume_de(p: &Path) -> String {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if s.len() >= 2 && s.as_bytes()[1] == b':' {
            return s[..2].to_uppercase();
        }
        String::new()
    }
    #[cfg(not(windows))]
    {
        let _ = p;
        String::new()
    }
}

pub fn espace_libre(p: &Path) -> u64 {
    fs2::available_space(p).unwrap_or(0)
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives, GetVolumeInformationW, SetFilePointerEx, WriteFile, FILE_BEGIN, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;

    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405C;
    const FSCTL_LOCK_VOLUME: u32 = 0x0009_0018;
    const FSCTL_UNLOCK_VOLUME: u32 = 0x0009_001C;
    const FSCTL_DISMOUNT_VOLUME: u32 = 0x0009_0020;

    pub fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    pub fn taille_source(f: &File) -> io::Result<u64> {
        // Un lecteur brut (\\.\E:) refuse les demandes d'informations de fichier (erreur 87) :
        // on demande d'abord la taille du disque, et on ne lit les métadonnées que pour un vrai fichier.
        if let Ok(m) = f.metadata() {
            if m.is_file() {
                return Ok(m.len());
            }
        }
        let mut longueur: i64 = 0;
        let mut ret: u32 = 0;
        let ok = unsafe {
            DeviceIoControl(
                f.as_raw_handle() as HANDLE,
                IOCTL_DISK_GET_LENGTH_INFO,
                std::ptr::null(),
                0,
                &mut longueur as *mut i64 as *mut _,
                8,
                &mut ret,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(longueur as u64)
    }

    pub fn taille_volume(drive: &str) -> u64 {
        let racine = wide(&format!("{}\\", drive.trim_end_matches('\\')));
        let (mut libre, mut total, mut tl) = (0u64, 0u64, 0u64);
        let ok = unsafe { GetDiskFreeSpaceExW(racine.as_ptr(), &mut libre, &mut total, &mut tl) };
        if ok == 0 {
            0
        } else {
            total
        }
    }

    pub fn lister_lecteurs() -> Vec<Lecteur> {
        let sys = std::env::var("SystemRoot").unwrap_or_default().get(..2).unwrap_or("").to_uppercase();
        let masque = unsafe { GetLogicalDrives() };
        let mut res = vec![];
        for i in 0..26u32 {
            if masque & (1 << i) == 0 {
                continue;
            }
            let l = format!("{}:", (b'A' + i as u8) as char);
            let racine = wide(&format!("{l}\\"));
            let t = unsafe { GetDriveTypeW(racine.as_ptr()) };
            if t != DRIVE_REMOVABLE && t != DRIVE_FIXED {
                continue;
            }
            let mut nom = [0u16; 261];
            let mut fsn = [0u16; 261];
            let (mut serie, mut maxlen, mut flags) = (0u32, 0u32, 0u32);
            let ok = unsafe { GetVolumeInformationW(racine.as_ptr(), nom.as_mut_ptr(), 261, &mut serie, &mut maxlen, &mut flags, fsn.as_mut_ptr(), 261) };
            let z = |b: &[u16]| String::from_utf16_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
            let (label, fs) = if ok != 0 { (z(&nom), z(&fsn)) } else { (String::new(), String::new()) };
            res.push(Lecteur {
                device: super::chemin_peripherique(&l),
                size: taille_volume(&l),
                removable: t == DRIVE_REMOVABLE,
                system: l == sys,
                letter: l,
                label,
                fs,
            });
        }
        res
    }

    pub fn est_admin() -> bool {
        unsafe {
            let mut tok: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut tok) == 0 {
                return false;
            }
            let mut el = TOKEN_ELEVATION { TokenIsElevated: 0 };
            let mut ret = 0u32;
            let ok = GetTokenInformation(tok, TokenElevation, &mut el as *mut _ as *mut _, std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut ret);
            CloseHandle(tok);
            ok != 0 && el.TokenIsElevated != 0
        }
    }

    pub fn relancer_admin() -> io::Result<()> {
        let exe = std::env::current_exe()?;
        let verbe = wide("runas");
        let fichier = wide(&exe.to_string_lossy());
        let dir = wide(&exe.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default());
        let r = unsafe { ShellExecuteW(std::ptr::null_mut(), verbe.as_ptr(), fichier.as_ptr(), std::ptr::null(), dir.as_ptr(), 1) };
        if (r as isize) <= 32 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "runas"));
        }
        Ok(())
    }

    /// Écrit des secteurs entiers sur un périphérique, verrouillé et démonté comme le fait TestDisk.
    pub fn ecrire_peripherique(cible: &str, off: u64, data: &[u8]) -> io::Result<()> {
        let p = wide(cible);
        unsafe {
            let h = CreateFileW(p.as_ptr(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, std::ptr::null(), OPEN_EXISTING, 0, std::ptr::null_mut());
            if h == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let mut ret = 0u32;
            let ioctl = |code: u32, ret: &mut u32| DeviceIoControl(h, code, std::ptr::null(), 0, std::ptr::null_mut(), 0, ret, std::ptr::null_mut());
            if ioctl(FSCTL_LOCK_VOLUME, &mut ret) == 0 {
                CloseHandle(h);
                return Err(io::Error::new(io::ErrorKind::Other, crate::i18n::l("impossible de verrouiller la carte : fermez les fenêtres et programmes qui l'utilisent, puis réessayez")));
            }
            ioctl(FSCTL_DISMOUNT_VOLUME, &mut ret);
            let mut res = Ok(());
            if SetFilePointerEx(h, off as i64, std::ptr::null_mut(), FILE_BEGIN) == 0 {
                res = Err(io::Error::last_os_error());
            } else {
                let mut n = 0u32;
                let mut t = crate::source::Aligne::new(data.len());
                t.as_mut().copy_from_slice(data);
                if WriteFile(h, t.as_mut().as_ptr(), data.len() as u32, &mut n, std::ptr::null_mut()) == 0 {
                    res = Err(io::Error::last_os_error());
                } else if n as usize != data.len() {
                    res = Err(io::Error::new(io::ErrorKind::WriteZero, crate::i18n::l("écriture incomplète")));
                } else {
                    FlushFileBuffers(h);
                }
            }
            ioctl(FSCTL_UNLOCK_VOLUME, &mut ret);
            CloseHandle(h);
            res
        }
    }
}

#[cfg(windows)]
pub use win::{est_admin, lister_lecteurs, relancer_admin, taille_source, taille_volume};

#[cfg(not(windows))]
pub fn taille_source(f: &File) -> io::Result<u64> {
    use std::io::{Seek, SeekFrom};
    let m = f.metadata()?;
    if m.is_file() {
        return Ok(m.len());
    }
    let mut g = f.try_clone()?;
    g.seek(SeekFrom::End(0))
}
#[cfg(not(windows))]
pub fn taille_volume(_drive: &str) -> u64 {
    0
}
#[cfg(not(windows))]
pub fn est_admin() -> bool {
    true
}
#[cfg(not(windows))]
pub fn relancer_admin() -> io::Result<()> {
    Ok(())
}
/// Hors Windows : liste vide, sauf lecteurs simulés pour tester l'interface (ATAECINA_FAKEDRIVES).
#[cfg(not(windows))]
pub fn lister_lecteurs() -> Vec<Lecteur> {
    if std::env::var("ATAECINA_FAKEDRIVES").is_err() {
        return vec![];
    }
    let l = |letter: &str, label: &str, fs: &str, size: u64, removable: bool, system: bool| Lecteur {
        letter: letter.into(),
        device: letter.into(),
        label: label.into(),
        fs: fs.into(),
        size,
        removable,
        system,
    };
    vec![
        l("C:", "Windows", "NTFS", 1 << 40, false, true),
        l("E:", "", "", 64 << 30, true, false),
        l("F:", "LUMIX_GH5_TOURNAGE_JOUR2", "exFAT", 128 << 30, true, false),
    ]
}

/// Écrit des secteurs : dans un fichier image, ou sur un périphérique (verrouillé sous Windows).
pub fn ecrire_secteurs(cible: &Path, off: u64, data: &[u8], peripherique: bool) -> io::Result<()> {
    #[cfg(windows)]
    if peripherique {
        return win::ecrire_peripherique(&cible.to_string_lossy(), off, data);
    }
    let _ = peripherique;
    let f = std::fs::OpenOptions::new().read(true).write(true).open(cible)?;
    crate::source::write_at(&f, data, off)?;
    f.sync_all()
}

/// Ouvre un dossier dans l'explorateur de fichiers.
pub fn ouvrir_dossier(p: &Path) {
    #[cfg(windows)]
    let mut c = Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut c = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut c = Command::new("xdg-open");
    let _ = c.arg(p).spawn();
}
