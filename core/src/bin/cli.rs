//! ataecina-cli : utilise le moteur sans interface (tests, scripts).
//!
//!   ataecina-cli analyse <image|carte> [--deep] [--copy <dossier>]
//!   ataecina-cli image <source> <image.dd>
//!   ataecina-cli restore <image>
//!   ataecina-cli repair --ref <ref.mov> --untrunc <untrunc> --out <dossier> <fichiers...>

use ataecina_core::{analyse, photorec::RecConfig, repair, tools};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;


fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Ok(l) = std::env::var("ATAECINA_LANG") {
        ataecina_core::i18n::set_lang(&l);
    }
    let stop = AtomicBool::new(false);
    match args.first().map(|s| s.as_str()) {
        Some("analyse") => {
            let src = PathBuf::from(&args[1]);
            let deep = args.iter().any(|a| a == "--deep");
            let copy = args.iter().position(|a| a == "--copy").map(|i| PathBuf::from(&args[i + 1]));
            let rc = RecConfig { source_kind: "image".into(), image: src.to_string_lossy().into(), deep, dest: copy.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(), ..Default::default() };
            let etat = Mutex::new(analyse::Etat { analyse_ok: false, ..Default::default() });
            let (source, drive, img_dir) = analyse::preparer(&rc).unwrap_or_else(|e| panic!("{e}"));
            let r = analyse::analyser(&rc, source, drive, &img_dir, &etat, &stop);
            {
                let e = etat.lock().unwrap();
                for l in &e.logs {
                    println!("  | {l}");
                }
                if let Err(err) = &r {
                    println!("ERREUR {err}");
                    std::process::exit(1);
                }
                for v in &e.vols {
                    println!("VOL {} {:?} boot={} main_ok={} backup_ok={} origine={} offset={}", serde_json::to_string(&v.kind).unwrap(), v.label, v.boot, v.main_ok, v.backup_ok, v.origin, v.offset);
                }
                for x in e.files.iter().filter(|x| !x.dir) {
                    println!("FILE {}{}{}/{} {} {}", if x.deleted { "DEL " } else { "" }, if x.orphan { "ORPH " } else { "" }, x.path, x.name, x.size, x.warn);
                }
            }
            if let Some(dest) = copy {
                etat.lock().unwrap().analyse_ok = true;
                let ids: Vec<usize> = etat.lock().unwrap().files.iter().filter(|x| !x.dir).map(|x| x.id).collect();
                let rc2 = RecConfig { dest: dest.to_string_lossy().into(), ..rc };
                let (liste, _, dir) = analyse::preparer_copie(&etat, &ids, &rc2).unwrap_or_else(|e| panic!("{e}"));
                let ffprobe = tools::trouver("ffprobe", &[]);
                analyse::copier(&liste, &dir, ffprobe.as_deref(), &etat, &stop).unwrap_or_else(|e| panic!("{e}"));
                for x in etat.lock().unwrap().files.iter().filter(|x| !x.dest.is_empty()) {
                    println!("COPIED {} {} {}", x.copied, x.name, x.dest);
                }
            }
        }
        Some("image") => {
            let mut der = String::new();
            let r = ataecina_core::imager::copier_image(&stop, &PathBuf::from(&args[1]), &PathBuf::from(&args[2]), &mut |e| {
                let s = format!("{} {}/{} bad={}", e.passe, e.fait, e.total, e.bad);
                if s != der {
                    der = s;
                }
            });
            println!("{der} -> {r:?}");
        }
        Some("restore") => {
            let img = PathBuf::from(&args[1]);
            let rc = RecConfig { source_kind: "image".into(), image: img.to_string_lossy().into(), ..Default::default() };
            let etat = Mutex::new(analyse::Etat::default());
            let (source, drive, d) = analyse::preparer(&rc).unwrap();
            analyse::analyser(&rc, source, drive, &d, &etat, &stop).unwrap();
            println!("{:?}", analyse::restaurer(&etat, 0, &img, false));
        }
        Some("repair") => {
            let get = |k: &str| args.iter().position(|a| a == k).map(|i| PathBuf::from(&args[i + 1]));
            let mut fichiers = vec![];
            let mut i = 1;
            while i < args.len() {
                if args[i].starts_with("--") {
                    i += 2;
                    continue;
                }
                fichiers.push(PathBuf::from(&args[i]));
                i += 1;
            }
            let job = repair::Job {
                references: vec![get("--ref").unwrap()],
                fichiers,
                sortie: get("--out").unwrap(),
                untrunc: get("--untrunc").unwrap(),
                ffprobe: tools::trouver("ffprobe", &[]).expect("ffprobe"),
                ffmpeg: tools::trouver("ffmpeg", &[]),
                verif_complete: args.iter().any(|a| a == "--verif"),
            };
            std::fs::create_dir_all(&job.sortie).unwrap();
            let etat = Mutex::new(repair::Etat::default());
            repair::executer(&job, &etat, &stop);
            for l in &etat.lock().unwrap().logs {
                println!("  | {l}");
            }
        }
        _ => eprintln!("usage : ataecina-cli analyse|image|restore|repair ..."),
    }
}
