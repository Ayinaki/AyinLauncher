import os
import glob

dirs_to_check = [
    'apps/app-frontend',
    'packages',
    'package.json',
    'README.md',
    'apps/CHANGELOG.md'
]

replacements = [
    ("MARCUSK Launcher", "Ayin Launcher"),
    ("MARCUSK", "Ayin"),
    ("marcusk", "ayin")
]

image_exts = {'.png', '.jpg', '.jpeg', '.svg', '.webp', '.gif', '.ico'}
images_found = []
files_modified = []

base_dir = r"d:\Devs\hippolytus-apps-v2.5.0"

for path in dirs_to_check:
    full_path = os.path.join(base_dir, path)
    if os.path.isfile(full_path):
        targets = [full_path]
    elif os.path.isdir(full_path):
        targets = []
        for root, _, files in os.walk(full_path):
            for file in files:
                if "node_modules" not in root and "dist" not in root:
                    targets.append(os.path.join(root, file))
    else:
        continue
        
    for target in targets:
        ext = os.path.splitext(target)[1].lower()
        if ext in image_exts:
            # Check filename
            if 'marcusk' in os.path.basename(target).lower():
                images_found.append(target)
            continue
            
        try:
            with open(target, 'r', encoding='utf-8') as f:
                content = f.read()
        except Exception:
            continue
            
        new_content = content
        for old, new in replacements:
            new_content = new_content.replace(old, new)
            
        if new_content != content:
            with open(target, 'w', encoding='utf-8') as f:
                f.write(new_content)
            files_modified.append(target)

print("Files modified:")
for f in files_modified:
    print(f)

print("\nImages found that might need manual replacement:")
for img in images_found:
    print(img)
