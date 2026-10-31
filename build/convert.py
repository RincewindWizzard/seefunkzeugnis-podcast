import re, html, sys
# Aufruf: python build/convert.py <skript.md> <template.html> <ausgabe.html>
import os
SRC, TMPL, OUT = sys.argv[1:4]
src = open(SRC, encoding='utf-8').read().split('\n')

ROLES = {
 'Skipper Hinnerk': ('hinnerk','Skipper Hinnerk','Seeadler'),
 'Jonas': ('jonas','Jonas','Seeadler'),
 'Tante Birte': ('birte','Tante Birte','Seeadler'),
 'Onkel Ole': ('ole','Onkel Ole','Seeadler'),
 'Skipper Klaas': ('klaas','Skipper Klaas','Möwe'),
 'Nordic Star': ('nordic','Kapitän Nordic Star','Frachter'),
 'Bremen Rescue': ('bremen','Bremen Rescue','Seenotleitung'),
 'Erzähler': ('erzaehler','Erzähler','')
}
e = html.escape
# start at Szene 1, stop at Spickzettel
start = next(i for i,l in enumerate(src) if l.startswith('# Szene 1'))
end = next(i for i,l in enumerate(src) if l.startswith('# Spickzettel'))
lines = src[start:end]
out = []
cur = None      # current role key
mode = None     # None / 'radio' / 'device'
buf = []
spk_open = False

def close_speaker():
    global spk_open
    if spk_open: out.append('</div></section>'); spk_open=False

def flush_device():
    global buf, mode
    if mode=='device':
        out.append('<div class="device"><div class="tag">⚙ Am Funkgerät</div><ul>' + ''.join(f'<li>{e(b)}</li>' for b in buf if b.strip()) + '</ul></div>')
        buf=[]; mode=None

def sfx(t):
    return f'<div class="sfx">🔊 {e(t)}</div>'

for raw in lines:
    s = raw.strip()
    ind = raw.startswith('\t')
    if mode=='radio':
        if s.startswith('-----'):
            body = '\n'.join(buf).strip('\n')
            out.append('<div class="radio"><div class="tag">📻 Funkspruch</div><pre>'+e(body)+'</pre></div>')
            buf=[]; mode=None
        else:
            buf.append(s)
        continue
    if mode=='device':
        if s=='' or s.startswith('-----') or (s.startswith('*') and s.endswith('*')):
            flush_device()
        else:
            buf.append(s); continue
    if s.startswith('# '):
        close_speaker()
        t = s[2:]
        n=re.search(r'Szene (\d+)',t); out.append(f'<h2 id="szene{n.group(1) if n else ""}">{e(t)}</h2>')
        continue
    if s.startswith('## '):
        close_speaker(); out.append(f'<h3>{e(s[3:])}</h3>'); continue
    if s=='': continue
    if s.startswith('-----'):
        mode='radio'; buf=[]; continue
    if s=='Am Funkgerät:':
        mode='device'; buf=[]; continue
    if s.startswith('*') and s.endswith('*'):
        if not ind: close_speaker()
        out.append(sfx(s.strip('*'))); continue
    m = re.match(r'^([^\t:]+?)(?:\s*\(([^)]*)\))?:\s*$', raw) if not ind else None
    if m and m.group(1) in ROLES:
        close_speaker()
        cls,name,ship = ROLES[m.group(1)]
        note = f' <span class="note">({e(m.group(2))})</span>' if m.group(2) else ''
        shiptxt = f'<span class="ship">{e(ship)}</span>' if ship else ''
        out.append(f'<section class="spk r-{cls}" data-role="{cls}"><div class="who">{e(name)}{note}{shiptxt}</div><div class="say">')
        spk_open=True; continue
    if not ind:
        close_speaker()
        # stage direction: sounds vs. scene info
        if re.search(r'(geräusch|tuckert|ruckt|Platsch|Möwen)', s):
            out.append(sfx(s))
        else:
            out.append(f'<div class="stage">🎬 {e(s)}</div>')
        continue
    # spoken line
    out.append(f'<p>{e(s)}</p>')
close_speaker()
body = '\n'.join(out)

cast = ''.join(f'<label class="chip r-{c}"><input type="checkbox" value="{c}"> {e(n)}{(" · "+e(sh)) if sh else ""}</label>' for c,n,sh in ROLES.values())
tmpl = open(TMPL, encoding='utf-8').read()
os.makedirs(os.path.dirname(OUT) or '.', exist_ok=True)
open(OUT,'w',encoding='utf-8').write(tmpl.replace('%%CAST%%',cast).replace('%%BODY%%',body))
print(body.count('class="radio"'), body.count('class="spk'))
