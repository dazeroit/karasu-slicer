#!/usr/bin/env python3
"""Independent protocol check; no production encoder/helper imports."""
import hashlib,json,struct,sys
from pathlib import Path

def preview_summary(previews):
 return [{'dimensions':[w,h],'rgb_sha256':hashlib.sha256(rgb).hexdigest(),
  'nonblack_pixels':sum(1 for i in range(0,len(rgb),3) if rgb[i:i+3]!=bytes(3)),
  'distinct_colors':len({rgb[i:i+3] for i in range(0,len(rgb),3)})} for w,h,rgb in previews]

def validate(path):
 return validate_with_previews(path)[0]

def validate_with_previews(path,decode_layers=True):
 """Return the summary and the decoded [small, large] previews as (width, height, RGB bytes).

 With decode_layers false only the header and previews are checked (fast preview inspection)."""
 b=Path(path).read_bytes()
 def unpack(fmt,p):
  n=struct.calcsize(fmt)
  if p<0 or p+n>len(b): raise ValueError('truncated field')
  return struct.unpack_from(fmt,b,p)
 def span(p,n):
  if p<0 or n<0 or p+n>len(b): raise ValueError('truncated image')
  return b[p:p+n]
 layers=[]
 if b[:4]==b'V3.0':
  magic=bytes.fromhex('07000000444c5000')
  assert b[4:12]==magic and b[-11:]==bytes(3)+magic
  at=194;previews=[]
  for side in (116,290):
   words=struct.unpack_from('>%dH'%(side*side),b,at)
   rgb=bytearray()
   # GOO thumbnails are RGB565: red 15..11, green 10..5, blue 4..0.
   for word in words:
    r,g,bl=word>>11,(word>>5)&63,word&31
    rgb.extend((r<<3|r>>2,g<<2|g>>4,bl<<3|bl>>2))
   previews.append((side,side,bytes(rgb)))
   assert b[at+side*side*2:at+side*side*2+2]==b'\r\n'
   at+=side*side*2+2
  count,w,h=unpack('>IHH',at)
  assert 0<w*h<=128*1024*1024 and 0<count<=1_000_000
  # Header tail has the explicit first layer pointer 7 bytes before end.
  assert unpack('>I',195470)[0]==195477
  p=195477
  for i in range(count if decode_layers else 0):
   pause,park,z,exposure=unpack('>Hfff',p)
   assert b[p+64:p+66]==b'\r\n'
   n=unpack('>I',p+66)[0]; data=span(p+70,n)
   assert data[0]==85 and data[-1]==(~sum(data[1:-1])&255)
   samples=bytearray(); j=1; previous=0
   while j<len(data)-1:
    tag=data[j];j+=1;kind=tag>>6
    if kind==2:
     value=previous+(-1 if tag&32 else 1)*(tag&15)
     run=data[j] if tag&16 else 1
     j+=bool(tag&16)
    else:
     value=(0,0,0,255)[kind]
     if kind==1:value=data[j];j+=1
     run=tag&15
     extra=(tag>>4)&3
     for k in range(extra):run|=data[j+k]<<(4+8*(extra-k-1))
     j+=extra
    assert 0<=value<=255 and run>0 and len(samples)+run<=w*h
    samples.extend(bytes([value])*run);previous=value
   assert len(samples)==w*h and j==len(data)-1
   assert b[p+70+n:p+72+n]==b'\r\n'
   layers.append({'z':z,'exposure':exposure,'pixels':list(samples) if w*h<=4096 else None,'mask_sha256':hashlib.sha256(samples).hexdigest()})
   p+=72+n
  assert p+11==len(b) or not decode_layers
  fmt='goo-v3'
 else:
  magic,version=unpack('<II',0)
  # Plain CTB v2/v3 (0x12FD0086), plain v4/v5 (0x12FD0106), Photon v1. Encrypted CTB
  # (0x12FD0107) is rejected: karasu never writes it.
  assert (magic,version) in [(0x12fd0086,2),(0x12fd0086,3),(0x12fd0106,4),(0x12fd0106,5),(0x12fd0019,1)]
  ctb=magic!=0x12fd0019
  w,h=unpack('<II',52);table,count=unpack('<II',64)
  assert 0<w*h<=128*1024*1024 and 0<count<=1_000_000
  assert unpack('<I',100)[0]==0
  previews=[]
  # Preview descriptors: large at 60, small at 72; zero means absent.
  for at in (72,60):
   descriptor=unpack('<I',at)[0]
   if not descriptor:continue
   pw,ph,data_at,n=unpack('<IIII',descriptor)
   assert 0<pw<=4096 and 0<ph<=4096 and n%2==0
   data=span(data_at,n);rgb=bytearray();j=0
   # RLE15 words: red 15..11, green 10..6, run flag 5, blue 4..0; a run is followed by
   # 0x3000 | (count - 1).
   while j<n:
    word=struct.unpack_from('<H',data,j)[0];j+=2
    run=1
    if word&0x20:
     assert j<n
     extra=struct.unpack_from('<H',data,j)[0];j+=2
     assert extra&0xf000==0x3000
     run=(extra&0xfff)+1
     assert run>=2
    r,g,bl=word>>11,(word>>6)&31,word&31
    assert len(rgb)//3+run<=pw*ph
    rgb.extend(bytes((r<<3|r>>2,g<<3|g>>2,bl<<3|bl>>2))*run)
   assert len(rgb)==pw*ph*3
   previews.append((pw,ph,bytes(rgb)))
  assert len(previews) in (0,2)
  for i in range(count if decode_layers else 0):
   z,exposure,off,offset,n=unpack('<fffII',table+i*36)
   if ctb and version>=3:
    # The row's page word is 0 and its LayerDefEx size 84; the 84-byte record before the image
    # starts with a copy of the row and holds 84 + image length.
    page,ex_size=unpack('<II',table+i*36+20)
    assert page==0 and ex_size==84
    assert offset>=84 and span(offset-84,36)==span(table+i*36,36)
    assert unpack('<I',offset-84+36)[0]==84+n
    if version>=4:assert off==0
   data=span(offset,n);samples=bytearray();j=0
   while j<len(data):
    code=data[j];j+=1
    if magic==0x12fd0019:value=255 if code&128 else 0;run=code&127
    else:
     value=code&127;value=2*value+1 if value else 0;run=1
     if code&128:
      q=data[j];j+=1
      if q<128:extra=0;run=q
      elif q<192:extra=1;run=q&63
      elif q<224:extra=2;run=q&31
      elif q<240:extra=3;run=q&15
      else:raise ValueError('invalid CTB count prefix')
      for _ in range(extra):run=run*256+data[j];j+=1
    assert run>0 and len(samples)+run<=w*h
    samples.extend(bytes([value])*run)
   assert len(samples)==w*h
   layers.append({'z':z,'exposure':exposure,'off':off,'pixels':list(samples) if w*h<=4096 else None,'mask_sha256':hashlib.sha256(samples).hexdigest()})
  fmt=f'ctb-v{version}-plain' if ctb else 'photon-v1'
  if ctb and version>=4:
   # Previews are mandatory; the slicer information points at the version-4 block, which points
   # at the 320-byte notice and (v5) the resin parameters.
   assert len(previews)==2
   slicer,slicer_size=unpack('<II',104)
   assert slicer_size==76
   block=unpack('<I',slicer+0x40)[0]
   notice,notice_len=unpack('<II',block+0x48)
   assert notice_len==320 and len(span(notice,320))==320
   assert unpack('<I',block+0x34)[0]==count
   resin=unpack('<I',block+0x50)[0]
   assert (resin!=0)==(version==5)
   if resin:
    for k in range(3):
     at,length=unpack('<II',resin+8+8*k)
     span(at,length)
 volume_ml=None
 if fmt.startswith('ctb-'):
  offset,size=unpack('<II',84)
  if offset and size>=24:volume_ml=unpack('<f',offset+20)[0]
 elif fmt=='goo-v3':volume_ml=unpack('>f',195450)[0]/1000.
 summary={'format':fmt,'dimensions':[w,h],'layers':layers,'encoded_coverage_volume_ml':volume_ml,'previews':preview_summary(previews)}
 return summary,previews

if __name__=='__main__':print(json.dumps(validate(sys.argv[1]),indent=2))
