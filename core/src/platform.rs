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

/// Programme lancé en arrière-plan (PhotoRec), qu'on peut surveiller et arrêter.
pub trait Processus: Send {
    /// None tant qu'il tourne, sinon Some(code de sortie).
    fn fini(&mut self) -> io::Result<Option<i64>>;
    fn tuer(&mut self);
}

impl Processus for std::process::Child {
    fn fini(&mut self) -> io::Result<Option<i64>> {
        Ok(self.try_wait()?.map(|s| s.code().map(|c| c as i64).unwrap_or(-1)))
    }
    fn tuer(&mut self) {
        let _ = self.kill();
        let _ = self.wait();
    }
}

/// Lance un programme en mode texte (PhotoRec) sans rien afficher.
/// Sous Windows, PhotoRec a besoin d'une vraie console : on lui en crée une, mais sa fenêtre reste masquée.
pub fn lancer_invisible(prog: &Path, args: &[std::ffi::OsString], dossier: &Path) -> io::Result<Box<dyn Processus>> {
    #[cfg(windows)]
    {
        win::lancer_masque(prog, args, dossier)
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new(prog);
        c.args(args).current_dir(dossier);
        Ok(Box::new(c.spawn()?))
    }
}

/// Construit une ligne de commande Windows (règles de guillemets de CommandLineToArgvW).
pub fn ligne_de_commande(prog: &Path, args: &[std::ffi::OsString]) -> String {
    let mut res = format!("\"{}\"", prog.display());
    for a in args {
        let a = a.to_string_lossy();
        res.push(' ');
        if !a.is_empty() && !a.contains([' ', '\t', '"']) {
            res.push_str(&a);
            continue;
        }
        res.push('"');
        let mut barres = 0;
        for ch in a.chars() {
            match ch {
                '\\' => barres += 1,
                '"' => {
                    res.push_str(&"\\".repeat(barres * 2 + 1));
                    res.push('"');
                    barres = 0;
                }
                _ => {
                    res.push_str(&"\\".repeat(barres));
                    res.push(ch);
                    barres = 0;
                }
            }
        }
        res.push_str(&"\\".repeat(barres * 2));
        res.push('"');
    }
    res
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

    struct ProcessusMasque {
        h: HANDLE,
    }
    unsafe impl Send for ProcessusMasque {}

    impl Processus for ProcessusMasque {
        fn fini(&mut self) -> io::Result<Option<i64>> {
            use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
            use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
            unsafe {
                if WaitForSingleObject(self.h, 0) != WAIT_OBJECT_0 {
                    return Ok(None);
                }
                let mut code = 0u32;
                if GetExitCodeProcess(self.h, &mut code) == 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(code as i32 as i64))
            }
        }
        fn tuer(&mut self) {
            use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
            unsafe {
                TerminateProcess(self.h, 1);
                WaitForSingleObject(self.h, 5000);
            }
        }
    }

    impl Drop for ProcessusMasque {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.h) };
        }
    }

    pub fn lancer_masque(prog: &Path, args: &[std::ffi::OsString], dossier: &Path) -> io::Result<Box<dyn Processus>> {
        use windows_sys::Win32::System::Threading::{CreateProcessW, CREATE_NEW_CONSOLE, PROCESS_INFORMATION, STARTF_USESHOWWINDOW, STARTUPINFOW};
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
        let mut ligne = wide(&super::ligne_de_commande(prog, args));
        let appli = wide(&prog.to_string_lossy());
        let rep = wide(&dossier.to_string_lossy());
        unsafe {
            let mut si: STARTUPINFOW = std::mem::zeroed();
            si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            si.dwFlags = STARTF_USESHOWWINDOW;
            si.wShowWindow = SW_HIDE as u16;
            let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
            let ok = CreateProcessW(appli.as_ptr(), ligne.as_mut_ptr(), std::ptr::null(), std::ptr::null(), 0, CREATE_NEW_CONSOLE, std::ptr::null(), rep.as_ptr(), &si, &mut pi);
            if ok == 0 {
                let e = io::Error::last_os_error();
                // 740 : le programme exige les droits administrateur (cas de photorec_win.exe).
                // Windows demande alors la confirmation, et le programme démarre élevé, toujours masqué.
                if e.raw_os_error() == Some(740) {
                    return lancer_eleve(prog, args, dossier);
                }
                return Err(e);
            }
            CloseHandle(pi.hThread);
            Ok(Box::new(ProcessusMasque { h: pi.hProcess }))
        }
    }

    fn lancer_eleve(prog: &Path, args: &[std::ffi::OsString], dossier: &Path) -> io::Result<Box<dyn Processus>> {
        use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
        // paramètres seuls : la ligne complète moins le programme en tête
        let complete = super::ligne_de_commande(prog, args);
        let entete = super::ligne_de_commande(prog, &[]);
        let params = wide(complete[entete.len()..].trim_start());
        let fichier = wide(&prog.to_string_lossy());
        let rep = wide(&dossier.to_string_lossy());
        let verbe = wide("runas");
        unsafe {
            let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
            info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
            info.lpVerb = verbe.as_ptr();
            info.lpFile = fichier.as_ptr();
            info.lpParameters = params.as_ptr();
            info.lpDirectory = rep.as_ptr();
            info.nShow = SW_HIDE;
            if ShellExecuteExW(&mut info) == 0 {
                let e = io::Error::last_os_error();
                if e.raw_os_error() == Some(1223) {
                    return Err(io::Error::new(io::ErrorKind::PermissionDenied, crate::i18n::l("droits administrateur refusés")));
                }
                return Err(e);
            }
            if info.hProcess.is_null() {
                return Err(io::Error::new(io::ErrorKind::Other, "ShellExecuteEx"));
            }
            Ok(Box::new(ProcessusMasque { h: info.hProcess }))
        }
    }

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

#[cfg(test)]
mod tests {
    use super::ligne_de_commande;
    use std::path::Path;

    #[test]
    fn guillemets_ligne_de_commande() {
        let a = |v: &[&str]| v.iter().map(|s| std::ffi::OsString::from(*s)).collect::<Vec<_>>();
        let l = ligne_de_commande(Path::new(r"C:\Program Files\testdisk\photorec_win.exe"), &a(&["/d", r"D:\Mes rushes\recup_dir", r"\\.\E:", "partition_none,search", r"D:\fin\", "a\"b", ""]));
        assert_eq!(l, r#""C:\Program Files\testdisk\photorec_win.exe" /d "D:\Mes rushes\recup_dir" \\.\E: partition_none,search D:\fin\ "a\"b" """#);
    }
}
