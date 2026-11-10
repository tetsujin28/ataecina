"""Captures d'écran d'Ataecina pour le mode d'emploi, dans les trois langues.

Les captures se font sur la vraie interface, pilotée par le vrai moteur (feature « serveur-test »),
sous Linux, avec des cartes de démonstration. Préparation (une fois) :

    sudo apt install libwebkit2gtk-4.1-dev mtools ffmpeg
    pip install playwright pillow && playwright install chromium
    cargo build -p ataecina --features serveur-test
    sudo DEMO=/ bash docs/tools/donnees_demo.sh target/debug/ataecina

puis :

    python docs/tools/captures.py          # fr en es
    python docs/tools/annoter.py           # cadres et zooms dans docs/guide/

donnees_demo.sh crée des dossiers « /C:/Ataecina » et « /D:/… » : les chemins s'affichent ensuite
comme sous Windows (C:\\Ataecina…). Les lecteurs E:, F: et G: sont simulés (ATAECINA_FAKEDRIVES) et
pointent vers des images de carte FAT32 : E: avec deux rushes supprimés, F: reformatée, G: avec un
secteur de démarrage effacé. Pour la réparation, un remplaçant d'untrunc (untrunc_demo.sh) produit
des fichiers réparés : il ne sert qu'aux captures et n'est jamais livré.
"""
import asyncio, json, os, pathlib, shutil, subprocess, sys, time
from playwright.async_api import async_playwright

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "docs" / "tools" / "out"
DEMO = pathlib.Path(os.environ.get("DEMO", "/"))
APPDIR = DEMO / "C:" / "Ataecina"
WORK = DEMO / "ataecina-demo"            # dossier de travail du serveur (lecteurs simulés E:, F:, G:)
TOOLS = WORK / "outils"                  # outils prêts à être copiés à côté de l'exe
CHROME = os.environ.get("CHROME")
SCALE = 1.5
W, H = 1440, 920
D = lambda *p: str(DEMO.joinpath("D:", *p))

DRIVES = {"admin": True, "drives": [
    {"device": "C:", "fs": "NTFS", "label": "Windows", "letter": "C:", "removable": False, "size": 1 << 40, "system": True},
    {"device": "D:", "fs": "NTFS", "label": "Données", "letter": "D:", "removable": False, "size": 2 << 40, "system": False},
    {"device": "E:", "fs": "FAT32", "label": "A001", "letter": "E:", "removable": True, "size": 1 << 30, "system": False},
    {"device": "F:", "fs": "FAT32", "label": "A001", "letter": "F:", "removable": True, "size": 1 << 30, "system": False},
    {"device": "G:", "fs": "", "label": "", "letter": "G:", "removable": True, "size": 1 << 30, "system": False}]}

# Éléments repérés sur chaque capture (expressions JavaScript qui renvoient un élément)
SEL = {
    "tabs": "q('#tabs')", "pills": "q('#tools')", "lang": "q('#langSel')",
    "srcSeg": "q('#srcSeg')", "drives": "q('#drives')", "drvE": "q('[data-drv=\"E:\"]')", "drvF": "q('[data-drv=\"F:\"]')",
    "drvG": "q('[data-drv=\"G:\"]')", "makeImage": "q('#makeImage')?.closest('label')", "methods": "q('#methods')",
    "optFs": "q('[data-meth=fs]')", "optPr": "q('[data-meth=photorec]')", "deep": "q('#deep')?.closest('label')",
    "recDest": "q('#recDest')?.closest('.field')", "autoSend": "q('#autoSend')?.closest('label')", "recStart": "q('#recStart')",
    "fmtChips": "q('#fmtChips')", "prHint": "q('#prHint')", "imagePath": "q('#imagePath')?.closest('.field')",
    "fBig": "q('#fBig')?.parentElement", "fSteps": "q('#fSteps')", "vol": "q('#fVols .vol')", "restoreBtn": "q('[data-restore]')",
    "volPill": "q('#fVols .pill2')", "fBarTools": "q('#fBarTools')", "fFilters": "q('#fBarTools button')?.parentElement",
    "fCopy": "q('#fCopy')", "fList": "q('#fList')", "delRow": "q('#fList .ft.del')?.closest('.frow')",
    "orphRow": "q('#fList .ft.orph')?.closest('.frow')", "copiedRow": "q('#fList .ft.ok')?.closest('.frow')", "fCur": "q('#fCur')",
    "restBox": "q('#restOv .box') || q('#restOv > div')", "restGo": "q('#restGo')", "restCancel": "q('#restCancel')",
    "refCard": "q('#refList')?.closest('.card')", "fileCard": "q('#fileList')?.closest('.card')", "outCard": "q('#sortie')?.closest('.card')",
    "untruncField": "q('#untrunc')?.closest('.field')", "ffmpegField": "q('#ffmpegDir')?.closest('.field')", "sortieField": "q('#sortie')?.closest('.field')",
    "startBtn": "q('#startBtn')", "stats": "q('#stats')", "results": "q('#results')", "resFail": "[...document.querySelectorAll('#results .res')].find(r=>/ECHEC|FAIL|FALL|chec/i.test(r.textContent))",
    "toast": "q('#toasts .toast')", "bigCount": "q('#bigCount')?.parentElement",
}
WIN = r"""(()=>{const re=/\/([A-Z]):((?:\/[^\s<>"'\/]*)*)/g;const x=s=>s.replace(/(Ataecina\\)(untrunc|ffmpeg|ffprobe|photorec)(?![\w.])/g,(m,a,n)=>a+(n==='photorec'?'photorec_win':n)+'.exe');const f=s=>x(s.replace(re,(m,d,r)=>d+':'+(r?r.replace(/\//g,'\\'):'\\')));
const w=document.createTreeWalker(document.body,NodeFilter.SHOW_TEXT);let n;while(n=w.nextNode()){re.lastIndex=0;if(re.test(n.nodeValue)){re.lastIndex=0;n.nodeValue=f(n.nodeValue)}}
document.querySelectorAll('input').forEach(i=>{if(i.value)i.value=f(i.value)})})()"""


def config(lang):
    return {"references": [D("References", "A001_11101407_C001.mov")],
            "files": [D("Rushes_a_reparer", f) for f in ["A001_11101414_C002.mov", "A001_11101435_C005.mov", "A001_11101449_C007.mov", "A001_11101463_C009.mov"]],
            "sortie": D("Reparation"), "untrunc": "", "ffmpegDir": "", "verif": False, "ignoreFtyp": False, "lastDir": "", "lang": lang,
            "rec": {"autoSend": False, "deep": False, "dest": D("Recuperation"), "drive": "E:", "formats": [], "freeOnly": False,
                    "image": D("Images", "A001.img"), "imageDir": D("Images"), "makeImage": False, "method": "fs", "photorec": "", "sourceKind": "drive"}}


def server(lang, outils):
    """Lance le serveur de test. outils=True : untrunc, ffmpeg, ffprobe et PhotoRec à côté de l'exe."""
    for n in ["untrunc", "ffmpeg", "ffprobe", "photorec"]:
        (APPDIR / n).unlink(missing_ok=True)
        if outils:
            shutil.copy2(TOOLS / n, APPDIR / n)
    # cartes remises à neuf (la restauration du format écrit sur G:)
    shutil.copy2(DEMO / "D:" / "Images" / "A001_abimee.orig", DEMO / "D:" / "Images" / "A001_abimee.img")
    shutil.rmtree(DEMO / "D:" / "Reparation", ignore_errors=True)
    (DEMO / "D:" / "Reparation").mkdir()
    cfg = WORK / "config.json"
    cfg.write_text(json.dumps(config(lang)))
    env = dict(os.environ, ATAECINA_HTTP="127.0.0.1:8080", ATAECINA_DEMO_WINDOWS="1", ATAECINA_FAKEDRIVES="1", ATAECINA_CONFIG=str(cfg), PATH="/nonexistent")
    p = subprocess.Popen([str(APPDIR / "Ataecina")], env=env, cwd=WORK, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.5)
    return p


async def shot(page, name):
    await page.wait_for_timeout(900)
    saved = await page.evaluate("[...document.querySelectorAll('input')].map(i=>i.value)")
    await page.evaluate(WIN)
    await page.screenshot(path=str(OUT / f"{name}.png"))
    # les champs reprennent leur vraie valeur, l'interface s'en sert pour lancer les traitements
    await page.evaluate("v=>document.querySelectorAll('input').forEach((i,k)=>{i.value=v[k]})", saved)
    r = {}
    for k, js in SEL.items():
        b = await page.evaluate(f"(()=>{{const q=s=>document.querySelector(s);const e={js};if(!e)return null;const r=e.getBoundingClientRect();if(!r.width||!r.height)return null;return [r.left,r.top,r.right,r.bottom]}})()")
        if b:
            r[k] = [round(v * SCALE) for v in b]
    json.dump(r, open(OUT / f"{name}.json", "w"))


async def until(page, js, limit=120):
    for _ in range(limit * 4):
        if await page.evaluate(js):
            return
        await page.wait_for_timeout(250)


async def open_page(b):
    page = await b.new_page(viewport={"width": W, "height": H}, device_scale_factor=SCALE)
    async def drives(route):
        await route.fulfill(status=200, content_type="application/json", body=json.dumps(DRIVES))
    await page.route("**/api/drives*", drives)
    await page.goto("http://127.0.0.1:8080/")
    await page.wait_for_timeout(1500)
    return page


async def phase_sans_outils(b, lang):
    page = await open_page(b)
    await shot(page, f"rec_start_{lang}")
    await page.click("#recStart")
    await until(page, "!!document.querySelector('#fCopy') && document.querySelector('#fCopy').offsetParent")
    await shot(page, f"rec_found_{lang}")
    await page.click("#fCopy")
    await until(page, "!!document.querySelector('#fList .ft.ok')")
    await page.wait_for_timeout(800)
    await shot(page, f"rec_copied_{lang}")
    await page.click('[data-drv="F:"]')
    await page.wait_for_timeout(400)
    await page.evaluate("(()=>{const d=document.querySelector('#deep'); if(!d.checked) d.click();})()")
    await page.wait_for_timeout(400)
    await page.click("#recStart")
    await until(page, "!!document.querySelector('#fList .ft.orph')")
    await page.wait_for_timeout(800)
    await shot(page, f"rec_deep_{lang}")
    await page.evaluate("(()=>{const d=document.querySelector('#deep'); if(d.checked) d.click();})()")
    await page.click('[data-drv="G:"]')
    await page.click("#recStart")
    await until(page, "!!document.querySelector('[data-restore]')")
    await page.wait_for_timeout(500)
    await shot(page, f"rec_boot_{lang}")
    await page.click("[data-restore]")
    await page.wait_for_timeout(600)
    await shot(page, f"rec_restore_{lang}")
    await page.click("#restCancel")
    await page.click("#srcSeg button:nth-child(2)")
    await page.wait_for_timeout(400)
    await shot(page, f"rec_image_{lang}")
    await page.click("#srcSeg button:nth-child(1)")
    await page.click("[data-meth=photorec]")
    await page.wait_for_timeout(400)
    await shot(page, f"rec_photorec_{lang}")
    await page.click("[data-tab=repair]")
    await page.wait_for_timeout(600)
    await shot(page, f"rep_missing_{lang}")
    await page.click("#startBtn")
    await page.wait_for_timeout(500)
    await shot(page, f"rep_error_{lang}")
    await page.close()


async def phase_avec_outils(b, lang):
    page = await open_page(b)
    await shot(page, f"rec_tools_{lang}")
    await page.click("[data-tab=repair]")
    await page.wait_for_timeout(600)
    await shot(page, f"rep_ready_{lang}")
    await page.click("#startBtn")
    await until(page, "document.querySelectorAll('#results .res').length >= 4 && !document.querySelector('#stopBtn').offsetParent")
    await page.wait_for_timeout(1000)
    await shot(page, f"rep_done_{lang}")
    await page.close()


async def main(lang):
    async with async_playwright() as p:
        b = await (p.chromium.launch(executable_path=CHROME) if CHROME else p.chromium.launch())
        for outils, phase in [(False, phase_sans_outils), (True, phase_avec_outils)]:
            srv = server(lang, outils)
            try:
                await phase(b, lang)
            finally:
                srv.kill()
                srv.wait()
        await b.close()


OUT.mkdir(exist_ok=True)
for l in (sys.argv[1:] or ["fr", "en", "es"]):
    asyncio.run(main(l))
