"""Annotations des captures du mode d'emploi de Ataecina : cadres orange (rouge pour les erreurs), pastilles numérotées, zooms.
Lance d'abord captures.py. """
from PIL import Image, ImageDraw, ImageFont
import json, os, pathlib
ROOT=pathlib.Path(__file__).resolve().parents[2]
OUT=ROOT/"docs"/"tools"/"out"; GUIDE=ROOT/"docs"/"guide"
ORANGE=(240,166,64); RED=(240,82,79)
F="/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
GUIDE.mkdir(exist_ok=True)
def union(bs): return [min(b[0] for b in bs),min(b[1] for b in bs),max(b[2] for b in bs),max(b[3] for b in bs)]
def frame(d,b,n,col,W,H,s=1.0,pad=8):
    pad=b[4] if len(b)>4 else pad
    p=pad*s; x0,y0,x1,y1=b[0]-p,b[1]-p,b[2]+p,b[3]+p
    x0=max(x0,3);y0=max(y0,3);x1=min(x1,W-4);y1=min(y1,H-4)
    d.rounded_rectangle((x0,y0,x1,y1),radius=int(14*s),outline=col,width=max(4,int(5*s)))
    if n:
        r=int(24*s)
        if len(b)>5 and b[5]=="left": cx=max(x0,r+6); cy=(y0+y1)/2
        elif len(b)>5 and b[5]=="right": cx=min(x1,W-r-6); cy=(y0+y1)/2
        elif len(b)>5 and b[5]=="top": cx=(x0+x1)/2; cy=y0-r-4
        elif (y1-y0)<90*s:
            cx=min(x1-r,W-r-6); cy=y1+r+4
            if cy+r>H: cy=y0-r-4
        else: cx=min(x1,W-r-6); cy=(y0+y1)/2
        d.ellipse((cx-r,cy-r,cx+r,cy+r),fill=col)
        f=ImageFont.truetype(F,int(28*s)); t=str(n); tw=d.textlength(t,font=f)
        d.text((cx-tw/2,cy-int(17*s)),t,font=f,fill=(20,16,5))
def dim(img,boxes,pad=8,alpha=115):
    ov=Image.new("L",img.size,alpha); od=ImageDraw.Draw(ov)
    for x0,y0,x1,y1 in boxes: od.rounded_rectangle((x0-pad,y0-pad,x1+pad,y1+pad),14,fill=0)
    return Image.composite(Image.new("RGB",img.size,(0,0,0)),img,ov)
def make(out,src,lang,marks,crop=None,margin=40,col=ORANGE,dimbg=True,maxw=1400):
    img=Image.open(OUT/f"{src}_{lang}.png").convert("RGB"); R=json.load(open(OUT/f"{src}_{lang}.json"))
    boxes=[list(R[k] if isinstance(k,str) else k(R))+list(m[2:]) for m in marks for k in [m[0]]]
    if dimbg and not crop: img=dim(img,[b[:4] for b in boxes])
    s=1.0
    if crop:
        c=union([(R[k] if isinstance(k,str) else k(R)) for k in crop])
        c=[max(0,c[0]-margin),max(0,c[1]-margin),min(img.width,c[2]+margin),min(img.height,c[3]+margin)]
        img=img.crop(c); s=min(2.0,maxw/img.width)
        img=img.resize((int(img.width*s),int(img.height*s)),Image.LANCZOS)
        boxes=[[(b[0]-c[0])*s,(b[1]-c[1])*s,(b[2]-c[0])*s,(b[3]-c[1])*s]+b[4:] for b in boxes]
    d=ImageDraw.Draw(img)
    for b,(n) in zip(boxes,[m[1] for m in marks]): frame(d,b,n,col,img.width,img.height,s=1.0 if not crop else max(1.0,s*0.8))
    if img.width>1440: img=img.resize((1440,round(img.height*1440/img.width)),Image.LANCZOS)
    img.quantize(256,method=Image.Quantize.MEDIANCUT,dither=Image.Dither.NONE).save(GUIDE/f"{out}_{lang}.png",optimize=True)
U=lambda *k:(lambda R:union([R[x] for x in k]))
for L in ["fr","en","es"]:
    # outils
    make("00_tools_missing","rep_missing",L,[("pills",1,8,"left"),("untruncField",2,8,"right")],col=RED)
    make("01_tools_error","rep_error",L,[("toast",1,8,"left"),("pills",2,8,"left")],col=RED)
    make("02_tools_ok","rep_ready",L,[("pills",1,8,"left"),("untruncField",2,8,"right")])
    make("03_tools_recovery","rec_tools",L,[("pills",None)],crop=["tabs","lang"],margin=30)
    make("04_photorec_missing","rec_photorec",L,[("pills",1,8,"left"),("optPr",2),("prHint",3,8,"left")])
    # récupération
    make("10_rec_start","rec_start",L,[("srcSeg",1,8,"left"),("drvE",2),("makeImage",3),("optFs",4),("deep",5),("recStart",6,8,"left")])
    make("11_rec_found","rec_found",L,[("vol",1,8,"left"),("fFilters",2,8,"left"),("delRow",3,4,"left"),("fCopy",4,8,"left")])
    make("12_rec_copied","rec_copied",L,[("fCur",1,8,"left"),("copiedRow",2,4,"left")])
    make("13_rec_deep","rec_deep",L,[("drvF",1),("deep",2),("orphRow",3,4,"left")])
    make("14_rec_boot","rec_boot",L,[("volPill",1,8,"left"),("restoreBtn",2,8,"right")])
    make("15_rec_restore","rec_restore",L,[("restBox",None)],dimbg=False)
    make("16_rec_image","rec_image",L,[("srcSeg",1,8,"left"),("imagePath",2,8,"left")])
    # réparation
    make("20_rep_ready","rep_ready",L,[("refCard",1,-4),("fileCard",2,-4),("outCard",3,-4),("startBtn",4,8,"left")])
    make("21_rep_done","rep_done",L,[("stats",1,8,"left"),("results",2,-4,"right")])
print("images écrites dans", GUIDE)
