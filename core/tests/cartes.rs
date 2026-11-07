//! Tests sur de vraies petites cartes exFAT et FAT32 (fixtures compressées) :
//! fichiers supprimés, secteur de démarrage effacé, carte reformatée, table de partitions effacée,
//! restauration du format et image avec zones illisibles.

use ataecina_core::analyse::{self, Etat};
use ataecina_core::fsread::{self, Fichier};
use ataecina_core::photorec::RecConfig;
use ataecina_core::source::{MemSource, Source};
use std::io::Read;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

fn charger(nom: &str) -> Vec<u8> {
    let p = format!("{}/tests/fixtures/{nom}.img.gz", env!("CARGO_MANIFEST_DIR"));
    let mut d = flate2::read::GzDecoder::new(std::fs::File::open(p).unwrap());
    let mut v = vec![];
    d.read_to_end(&mut v).unwrap();
    v
}

/// Contenu attendu des rushes de test.
fn attendu(i: usize) -> Vec<u8> {
    let motif = format!("CLIP{i}-").into_bytes();
    motif.iter().cycle().take(300_000 + i * 1000).copied().collect()
}

fn lire_tout(img: &[u8], deep: bool) -> (Vec<fsread::Volume>, Vec<Fichier>) {
    let r = MemSource(img.to_vec());
    let stop = AtomicBool::new(false);
    let mut vols = fsread::trouver_volumes(&r, &stop, &mut |_| {});
    let mut tous = vec![];
    for v in vols.iter_mut() {
        let mut fs = v.lire(&r);
        if deep {
            let o = v.recherche_approfondie(&r, &fs, &stop, &mut |_, _| {});
            fs.extend(o);
        }
        tous.extend(fs);
    }
    (vols, tous)
}

fn contenu(img: &[u8], f: &Fichier) -> Vec<u8> {
    let r = MemSource(img.to_vec());
    let mut v = vec![];
    for &(p, n) in &f.runs {
        v.extend(r.lire(p, n as usize));
    }
    v
}

/// Vérifie que les 5 rushes sont retrouvés avec leur nom et un contenu identique.
fn verifier_rushes(img: &[u8], fichiers: &[Fichier]) {
    for i in 1..=5 {
        let nom = format!("P100000{i}.MOV");
        let f = fichiers.iter().find(|f| f.name == nom).unwrap_or_else(|| panic!("{nom} introuvable"));
        assert_eq!(contenu(img, f), attendu(i), "contenu de {nom}");
    }
}

#[test]
fn exfat_fichiers_et_supprimes() {
    let img = charger("exfat");
    let (vols, fs) = lire_tout(&img, false);
    assert_eq!(vols.len(), 1);
    assert_eq!(vols[0].label, "TESTCARD");
    assert!(vols[0].main_ok && vols[0].backup_ok);
    verifier_rushes(&img, &fs);
    let del: Vec<_> = fs.iter().filter(|f| f.deleted && !f.dir).map(|f| f.name.as_str()).collect();
    assert_eq!(del.len(), 2);
    assert!(fs.iter().any(|f| f.name == "Nom très long avec accents.txt"));
    assert!(fs.iter().all(|f| f.name != "P1000001.MOV" || f.path == "DCIM/100_PANA"));
}

#[test]
fn exfat_secteur_demarrage_efface() {
    let mut img = charger("exfat");
    img[..512].fill(0);
    let (vols, fs) = lire_tout(&img, false);
    assert_eq!(vols.len(), 1);
    assert!(!vols[0].main_ok && vols[0].backup_ok);
    verifier_rushes(&img, &fs);
}

#[test]
fn exfat_table_partitions_effacee() {
    let carte = charger("exfat");
    // disque partitionné (volume à 1 Mio), MBR et secteur principal effacés
    let mut img = vec![0u8; carte.len() + (2 << 20)];
    img[1 << 20..(1 << 20) + carte.len()].copy_from_slice(&carte);
    img[1 << 20..(1 << 20) + 512].fill(0);
    let (vols, fs) = lire_tout(&img, false);
    assert_eq!(vols.len(), 1);
    assert_eq!(vols[0].offset, 1 << 20);
    verifier_rushes(&img, &fs);
}

#[test]
fn exfat_reformatee_recherche_approfondie() {
    let img = charger("exfat_format");
    let (_, sans) = lire_tout(&img, false);
    assert!(sans.iter().all(|f| !f.name.ends_with(".MOV")));
    let (_, avec) = lire_tout(&img, true);
    verifier_rushes(&img, &avec);
    assert!(avec.iter().filter(|f| f.name.ends_with(".MOV")).all(|f| f.orphan));
}

#[test]
fn fat32_supprimes_et_initiale() {
    let img = charger("fat32");
    let (vols, fs) = lire_tout(&img, false);
    assert_eq!(vols[0].label, "TESTCARD");
    // la première lettre des fichiers supprimés est déduite des voisins
    verifier_rushes(&img, &fs);
}

#[test]
fn fat32_reformatee() {
    let img = charger("fat32_format");
    let (_, fs) = lire_tout(&img, true);
    verifier_rushes(&img, &fs);
}

#[test]
fn restauration_du_format() {
    let dir = std::env::temp_dir().join(format!("ataecina_test_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("carte.img");
    let mut img = charger("exfat");
    let original = img[..512].to_vec();
    img[..512].fill(0);
    std::fs::write(&p, &img).unwrap();
    let rc = RecConfig { source_kind: "image".into(), image: p.to_string_lossy().into(), ..Default::default() };
    let etat = Mutex::new(Etat::default());
    let stop = AtomicBool::new(false);
    let (src, drive, d) = analyse::preparer(&rc).unwrap();
    analyse::analyser(&rc, src, drive, &d, &etat, &stop).unwrap();
    analyse::restaurer(&etat, 0, &p, false).unwrap();
    let apres = std::fs::read(&p).unwrap();
    // le secteur principal est maintenant la copie de secours (secteur 12)...
    assert_eq!(&apres[..512], &apres[12 * 512..13 * 512]);
    // ... identique à l'original hormis les champs que seul le principal tient à jour
    // (VolumeFlags 106-107, PercentInUse 112)
    for i in (0..512).filter(|i| ![106, 107, 112].contains(i)) {
        assert_eq!(apres[i], original[i], "octet {i}");
    }
    // et la carte est de nouveau valide sans recourir au secours
    let (vols, _) = lire_tout(&apres, false);
    assert!(vols[0].main_ok);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn image_avec_zones_illisibles_et_reprise() {
    let dir = std::env::temp_dir().join(format!("ataecina_img_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("carte.img");
    let dst = dir.join("carte.dd");
    let img = charger("exfat");
    std::fs::write(&src, &img).unwrap();
    // zone morte de 64 Ko à 4 Mio, zone fragile (1 échec par secteur) à 8 Mio
    std::env::set_var("ATAECINA_BADRANGES", format!("{}:65536:0,{}:8192:1", 4 << 20, 8 << 20));
    let stop = AtomicBool::new(false);
    let mut der = Default::default();
    ataecina_core::imager::copier_image(&stop, &src, &dst, &mut |e| der = e).unwrap();
    std::env::remove_var("ATAECINA_BADRANGES");
    assert_eq!(der.bad, 65536);
    let out = std::fs::read(&dst).unwrap();
    for (i, (a, b)) in img.chunks(512).zip(out.chunks(512)).enumerate() {
        let pos = i as u64 * 512;
        if (4 << 20..(4 << 20) + 65536).contains(&pos) {
            assert!(b.iter().all(|&x| x == 0));
        } else {
            assert_eq!(a, b, "secteur {i}");
        }
    }
    // le fichier de suivi permet une reprise : une relance ne recopie rien et garde la zone morte
    assert!(dir.join("carte.dd.map").exists());
    let _ = std::fs::remove_dir_all(dir);
}
