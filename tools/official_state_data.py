#!/usr/bin/env python3
from __future__ import annotations
import argparse, hashlib, json, os, re, shutil, subprocess, sys, tempfile, urllib.error, urllib.request, zipfile
from pathlib import Path

MANIFEST_URL='https://piston-meta.mojang.com/mc/game/version_manifest_v2.json'
TARGET_PROTOCOL=776
TARGET_DATA_VERSION=4903
CLASS_NAMES={
 'shared_constants':'net.minecraft.SharedConstants',
 'bootstrap':'net.minecraft.server.Bootstrap',
 'blocks':'net.minecraft.world.level.block.Blocks',
 'block':'net.minecraft.world.level.block.Block',
 'block_state':'net.minecraft.world.level.block.state.BlockState',
 'state_base':'net.minecraft.world.level.block.state.BlockBehaviour$BlockStateBase',
 'fluid_state':'net.minecraft.world.level.material.FluidState',
}

def sha256_file(path: Path)->str:
 h=hashlib.sha256()
 with path.open('rb') as f:
  for chunk in iter(lambda:f.read(1<<20),b''): h.update(chunk)
 return h.hexdigest()

def sha1_file(path: Path)->str:
 h=hashlib.sha1()
 with path.open('rb') as f:
  for chunk in iter(lambda:f.read(1<<20),b''): h.update(chunk)
 return h.hexdigest()

def fetch_json(url:str):
 with urllib.request.urlopen(url, timeout=30) as r: return json.load(r)

def download(url:str,path:Path,expected_sha1:str|None=None,expected_size:int|None=None):
 path.parent.mkdir(parents=True,exist_ok=True)
 if not path.exists():
  with urllib.request.urlopen(url,timeout=60) as r, path.open('wb') as f: shutil.copyfileobj(r,f)
 if expected_size is not None and path.stat().st_size!=expected_size: raise ValueError(f'size mismatch for {path}')
 if expected_sha1 is not None and sha1_file(path)!=expected_sha1: raise ValueError(f'SHA-1 mismatch for {path}')

def resolve(version:str,cache:Path):
 manifest=fetch_json(MANIFEST_URL)
 entry=next((v for v in manifest['versions'] if v['id']==version),None)
 if entry is None: raise ValueError(f'official launcher manifest has no version {version}')
 meta=fetch_json(entry['url']); result={}
 asset=meta['downloads']['server']; path=cache/f'{version}-server.bin'; download(asset['url'],path,asset.get('sha1'),asset.get('size')); result['server']=(path,asset)
 if 'server_mappings' in meta['downloads']:
  asset=meta['downloads']['server_mappings']; path=cache/f'{version}-server_mappings.bin'; download(asset['url'],path,asset.get('sha1'),asset.get('size')); result['server_mappings']=(path,asset)
 return result,meta

def parse_mappings(text:str):
 classes={}; fields={}; methods={}; current=None
 for raw in text.splitlines():
  if not raw.strip(): continue
  if not raw[0].isspace() and ' -> ' in raw and raw.rstrip().endswith(':'):
   left,right=raw.rstrip()[:-1].split(' -> ',1); current=left.strip(); classes[current]=right.strip(); continue
  if current is None or ' -> ' not in raw: continue
  left,right=raw.strip().rsplit(' -> ',1); left=re.sub(r'^\d+:\d+:','',left); left=re.sub(r':\d+:\d+$','',left)
  if '(' in left:
   name=left.split('(',1)[0].strip().split()[-1]; methods.setdefault((current,name),set()).add(right.strip())
  else:
   parts=left.split()
   if len(parts)>=2: fields[(current,parts[-1])]=right.strip()
 return classes,fields,methods

def unique_method(methods,owner,name):
 values=methods.get((owner,name),set())
 if len(values)!=1: raise ValueError(f'expected unique mapping for {owner}#{name}, got {sorted(values)}')
 return next(iter(values))

def extract_classpath(bundle:Path,root:Path):
 root.mkdir(parents=True,exist_ok=True); entries=[]
 with zipfile.ZipFile(bundle) as zf:
  jars=[n for n in zf.namelist() if n.endswith('.jar') and (n.startswith('META-INF/versions/') or n.startswith('META-INF/libraries/'))]
  if not jars: return [bundle]
  for i,name in enumerate(jars):
   path=root/f'{i:04d}-{Path(name).name}'; path.write_bytes(zf.read(name)); entries.append(path)
 return entries

def java_probe(mapping_text:str|None):
 if mapping_text is None:
  obf=dict(CLASS_NAMES); field='BLOCK_STATE_REGISTRY'; names={'detect_version':'tryDetectVersion','bootstrap':'bootStrap','get_id':'getId','is_air':'isAir','random_block':'isRandomlyTicking','get_fluid':'getFluidState','fluid_empty':'isEmpty','random_fluid':'isRandomlyTicking'}
  return _java_source(obf,field,names)
 classes,fields,methods=parse_mappings(mapping_text); obf={key:classes[value] for key,value in CLASS_NAMES.items()}; field=fields[(CLASS_NAMES['block'],'BLOCK_STATE_REGISTRY')]
 names={
  'detect_version':unique_method(methods,CLASS_NAMES['shared_constants'],'tryDetectVersion'),
  'bootstrap':unique_method(methods,CLASS_NAMES['bootstrap'],'bootStrap'),
  'get_id':unique_method(methods,CLASS_NAMES['block'],'getId'),
  'is_air':unique_method(methods,CLASS_NAMES['state_base'],'isAir'),
  'random_block':unique_method(methods,CLASS_NAMES['state_base'],'isRandomlyTicking'),
  'get_fluid':unique_method(methods,CLASS_NAMES['state_base'],'getFluidState'),
  'fluid_empty':unique_method(methods,CLASS_NAMES['fluid_state'],'isEmpty'),
  'random_fluid':unique_method(methods,CLASS_NAMES['fluid_state'],'isRandomlyTicking'),
 }
 return _java_source(obf,field,names)

def _java_source(obf,field,names):
 return f'''import java.lang.reflect.*;\npublic final class CrucibleStateProbe {{\n  static Method zero(Class<?> c,String n) throws Exception {{ Method m=c.getMethod(n); m.setAccessible(true); return m; }}\n  static Method oneStatic(Class<?> c,String n) throws Exception {{ for(Method m:c.getMethods()) if(m.getName().equals(n)&&Modifier.isStatic(m.getModifiers())&&m.getParameterCount()==1) {{m.setAccessible(true);return m;}} throw new NoSuchMethodException(n); }}\n  public static void main(String[] args) throws Exception {{\n    Class<?> constants=Class.forName("{obf['shared_constants']}"); zero(constants,"{names['detect_version']}").invoke(null);\n    Class<?> bootstrap=Class.forName("{obf['bootstrap']}"); zero(bootstrap,"{names['bootstrap']}").invoke(null);\n    Class.forName("{obf['blocks']}",true,CrucibleStateProbe.class.getClassLoader());\n    Class<?> block=Class.forName("{obf['block']}");\n    Field registryField=block.getField("{field}"); registryField.setAccessible(true); Object registry=registryField.get(null);\n    Method getId=oneStatic(block,"{names['get_id']}");\n    Class<?> stateBase=Class.forName("{obf['state_base']}"); Method isAir=zero(stateBase,"{names['is_air']}"); Method randomBlock=zero(stateBase,"{names['random_block']}"); Method getFluid=zero(stateBase,"{names['get_fluid']}");\n    Class<?> fluidState=Class.forName("{obf['fluid_state']}"); Method fluidEmpty=zero(fluidState,"{names['fluid_empty']}"); Method randomFluid=zero(fluidState,"{names['random_fluid']}");\n    for(Object state:(Iterable<?>)registry) {{\n      int id=((Number)getId.invoke(null,state)).intValue(); boolean air=(Boolean)isAir.invoke(state); boolean rb=!air && (Boolean)randomBlock.invoke(state); Object fluid=getFluid.invoke(state); boolean cf=!air && !(Boolean)fluidEmpty.invoke(fluid); boolean rf=cf && (Boolean)randomFluid.invoke(fluid);\n      String raw=state.toString().replace("\\t"," ").replace("\\n"," ");\n      System.out.println(id+"\\t"+(air?0:1)+"\\t"+(cf?1:0)+"\\t"+(rb?1:0)+"\\t"+(rf?1:0)+"\\t"+raw);\n    }}\n  }}\n}}\n'''

def canonical_key(raw:str)->str:
 m=re.fullmatch(r'Block\{([^}]+)\}(?:\[(.*)\])?',raw)
 if not m: raise ValueError(f'unexpected BlockState string: {raw}')
 block=m.group(1); props=m.group(2)
 if not props: return block
 return block+'['+','.join(sorted(p for p in props.split(',') if p))+']'

def run_probe(server:Path,mappings:Path|None,work:Path):
 cp_entries=extract_classpath(server,work/'classpath'); mapping_text=mappings.read_text(encoding='utf-8') if mappings else None; source=java_probe(mapping_text); java=work/'CrucibleStateProbe.java'; java.write_text(source)
 cp=os.pathsep.join(str(p) for p in cp_entries); compile_result=subprocess.run(['javac','-encoding','UTF-8','-d',str(work),str(java)],text=True,capture_output=True)
 if compile_result.returncode!=0: raise ValueError(f'probe compilation failed:\n{compile_result.stderr}\n{compile_result.stdout}')
 result=subprocess.run(['java','-cp',str(work)+os.pathsep+cp,'CrucibleStateProbe'],text=True,capture_output=True)
 if result.returncode!=0: raise ValueError(f'probe runtime failed (exit {result.returncode}):\nSTDERR:\n{result.stderr}\nSTDOUT:\n{result.stdout}')
 states=[]
 for line in result.stdout.splitlines():
  if not line.strip(): continue
  parts=line.split('\t',5)
  if len(parts)!=6: raise ValueError(f'invalid probe line: {line}')
  vid,na,cf,rb,rf,raw=parts; states.append({'key':canonical_key(raw),'vanilla_id':int(vid),'non_air':na=='1','counted_fluid':cf=='1','random_block':rb=='1','random_fluid':rf=='1'})
 states.sort(key=lambda s:s['vanilla_id']); ids=[s['vanilla_id'] for s in states]
 if ids!=list(range(len(states))): raise ValueError('official BLOCK_STATE_REGISTRY is not dense in probe output')
 return states

def extract(version:str,output:Path,cache:Path,server:Path|None,mappings:Path|None):
 if server is None:
  resolved,_=resolve(version,cache); server=resolved['server'][0]
  if mappings is None and 'server_mappings' in resolved: mappings=resolved['server_mappings'][0]
 assert server is not None
 with tempfile.TemporaryDirectory(prefix='crucible-state-probe-') as td: states=run_probe(server,mappings,Path(td))
 data={'schema':1,'target':{'minecraft_version':version,'protocol_version':TARGET_PROTOCOL,'data_version':TARGET_DATA_VERSION},'air_key':'minecraft:air','provenance':{'server_sha256':sha256_file(server),'server_mappings_sha256':sha256_file(mappings) if mappings else None,'name_mapping':'proguard' if mappings else 'identity-unobfuscated','startup_sequence':['SharedConstants.tryDetectVersion','Bootstrap.bootStrap'],'source':'official-runtime-reflection-probe-v1'},'states':states}
 output.parent.mkdir(parents=True,exist_ok=True); output.write_text(json.dumps(data,indent=2,sort_keys=True)+'\n'); print(f'extracted {len(states)} official block states -> {output}')

def main():
 p=argparse.ArgumentParser(); p.add_argument('--version',default='26.2'); p.add_argument('--output',required=True); p.add_argument('--cache',default='.crucible/vanilla/downloads'); p.add_argument('--server-jar'); p.add_argument('--server-mappings'); a=p.parse_args()
 try:
  extract(a.version,Path(a.output),Path(a.cache),Path(a.server_jar) if a.server_jar else None,Path(a.server_mappings) if a.server_mappings else None); return 0
 except (ValueError,KeyError,subprocess.CalledProcessError,OSError,urllib.error.URLError) as e:
  print(f'error: {e}',file=sys.stderr); return 2
if __name__=='__main__': raise SystemExit(main())
