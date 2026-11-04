import sys, struct, io
TYPES=["END","PLAYER","TEMPLATE","DEFINITION","DEFINITION_LINK","SKIN","SKIN_LINK","PLAYER_PARTPOS","RENDER_EFFECT","UUID_LOCK","ANIMATION_DATA","SKIN_TYPE","MODEL_ROOT","LIST_ICON","DUP_ROOT","CLONEABLE","SCALE","TEXTURE","ANIMATED_TEX","TAGS","PACKAGE_LINK","CUBES","ROOT_INFO","ANIMATION_NEW"]
DIV=32767//48
class R:
    def __init__(s,b): s.b=b; s.p=0
    def u8(s): v=s.b[s.p]; s.p+=1; return v
    def i8(s): v=struct.unpack_from('b',s.b,s.p)[0]; s.p+=1; return v
    def i16(s): v=struct.unpack_from('>h',s.b,s.p)[0]; s.p+=2; return v
    def var(s):
        i=j=0
        while True:
            b=s.u8(); i|=(b&127)<<(7*j); j+=1
            if not b&128: return i
    def svar(s):
        b=s.u8(); sign=-1 if b&64 else 1; i=b&63; j=6
        while b&128: b=s.u8(); i|=(b&127)<<j; j+=7
        return i*sign
    def raw(s,n): v=s.b[s.p:s.p+n]; s.p+=n; return v
    def utf(s): return s.raw(s.var()).decode()
    def block(s): return R(s.raw(s.var()))
    def vec3ub(s): return [s.u8()/10 for _ in range(3)]
    def vec6b(s): return [s.i16()/DIV for _ in range(3)]
    def angle(s): return [s.i16()/65535*360 for _ in range(3)]
    def left(s): return len(s.b)-s.p
def parts(r, depth):
    while r.left():
        t=r.u8(); blk=r.block(); name=TYPES[t] if t<len(TYPES) else t
        print("  "*depth+f"{name} ({len(blk.b)} bytes)")
        if name=="END": return
        if name=="DEFINITION":
            n=blk.var(); print("  "*depth+f" cubes={n}")
            for i in range(n):
                c=dict(size=blk.vec3ub(),pos=blk.vec6b(),off=blk.vec6b(),rot=blk.angle(),parent=blk.var())
                tex=blk.i8()
                if tex==0: c['rgb']=blk.raw(3).hex()
                else: c['tex']=tex; c['uv']=(blk.u8(),blk.u8())
                print("  "*depth+f"  #{i+10} {c}")
            parts(blk, depth+1)
        elif name=="SKIN":
            print("  "*depth+f" size={blk.i16()},{blk.i16()} png={blk.var()}")
        elif name=="PLAYER": print("  "*depth+f" keep={blk.u8():07b}")
        elif name=="PLAYER_PARTPOS": print("  "*depth+f" id={blk.var()} pos={blk.vec6b()} rot={blk.angle()}")
        else: print("  "*depth+"  "+blk.b[:64].hex())
b=open(sys.argv[1],'rb').read(); r=R(b)
assert r.u8()==0x53
print(r.utf(), '|', r.utf())
d=r.block(); assert d.u8()==0x53
parts(d,1); print("trailing in data:",d.left(), d.b[d.p:].hex())
print("after data:", r.var(), "icon bytes:", len(r.block().b), "left", r.left())
