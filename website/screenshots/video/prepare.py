import plistlib, shutil, subprocess, sys
from pathlib import Path
import os; REPO=Path(os.environ['OFF42_REPO']); out=Path(sys.argv[1])
for app in ['PDF42','DOC42','XLS42','PPT42','IMG42','SCN42']:
    b=out/f'{app}Audit.app'; c=b/'Contents'; (c/'MacOS').mkdir(parents=True,exist_ok=True); (c/'Resources').mkdir(exist_ok=True)
    src=REPO/'.build/debug'/app; shutil.copy2(src,c/'MacOS'/app)
    info=plistlib.load(open(REPO/'Resources'/app/'Info.plist','rb'))
    rb=src.parent/'Suite42_Kit42.bundle'; d=c/'Resources/Suite42_Kit42.bundle'
    if d.exists(): shutil.rmtree(d)
    shutil.copytree(rb,d)
    info['CFBundleIdentifier']=f'com.sfzorin.uiaudit.{app.lower()}'; info['CFBundleName']=info['CFBundleDisplayName']=app+' Audit'
    plistlib.dump(info,open(c/'Info.plist','wb')); shutil.copy2(REPO/'Resources'/app/'AppIcon.icns',c/'Resources/AppIcon.icns')
    subprocess.run(['codesign','--force','-s','-',str(b)],check=True,capture_output=True); print(app,'ok')
