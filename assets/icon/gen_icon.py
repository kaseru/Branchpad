from PIL import Image, ImageDraw, ImageChops
import sys
S=1024; K=3; N=S*K
def lerp(a,b,t): return tuple(int(a[i]+(b[i]-a[i])*t) for i in range(3))
# diagonal gradient
bg=Image.new("RGB",(N,N)); px=bg.load()
c1,c2=(99,102,241),(14,165,233)  # indigo -> sky
g=Image.linear_gradient("L").resize((N,N)).rotate(45,resample=Image.BICUBIC,expand=False)
bg=Image.composite(Image.new("RGB",(N,N),c2),Image.new("RGB",(N,N),c1),g)
# squircle mask (macOS-like, 824/1024 body)
m=Image.new("L",(N,N),0); d=ImageDraw.Draw(m)
pad=int(N*0.098); d.rounded_rectangle([pad,pad,N-pad,N-pad],radius=int(N*0.185),fill=255)
fg=Image.new("RGBA",(N,N),(0,0,0,0)); f=ImageDraw.Draw(fg)
W=(255,255,255,255); u=N/100
def P(x,y): return (x*u,y*u)
lw=int(5.2*u)
# trunk and branch
f.line([P(38,26),P(38,74)],fill=W,width=lw)
f.line([P(38,58),P(38,52),P(62,40),P(62,34)] if False else [P(38,60),P(62,44),P(62,34)],fill=W,width=lw,joint="curve")
r=7.2*u
for (x,y) in [(38,24),(38,76),(62,30)]:
    cx,cy=x*u,y*u
    f.ellipse([cx-r,cy-r,cx+r,cy+r],fill=(255,255,255,255))
    r2=3.2*u
    f.ellipse([cx-r2,cy-r2,cx+r2,cy+r2],fill=(79,70,229,255))
# round line ends
for (x,y) in [(38,26),(38,74)]: pass
out=Image.new("RGBA",(N,N),(0,0,0,0)); out.paste(bg,(0,0),m)
# soft shadow under glyph
sh=Image.new("RGBA",(N,N),(0,0,0,0)); sh.paste((30,27,75,90),(0,0),fg.split()[3])
from PIL import ImageFilter
sh=sh.filter(ImageFilter.GaussianBlur(10*K)); sh=ImageChops.offset(sh,0,8*K)
sm=Image.new("RGBA",(N,N),(0,0,0,0)); sm.paste(sh,(0,0),m)
out=Image.alpha_composite(out,sm); out=Image.alpha_composite(out,fg)
out=out.resize((S,S),Image.LANCZOS)
out.save("icon.png")
out.save("icon.ico",sizes=[(256,256),(128,128),(64,64),(48,48),(32,32),(16,16)])
Image.alpha_composite(Image.new("RGBA",(S,S),(255,255,255,255)),out).resize((256,256)).save("prev.png")
