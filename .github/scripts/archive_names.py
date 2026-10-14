"""Resolve CI artifact filenames from owning package manifests."""
import json
import os
from pathlib import Path
import re
import tomllib

def archive_names(root):
    contract = json.loads((root / '.github/registry-release.json').read_text())
    result = {}
    for item in contract['packages']:
        document = json.loads((root / item['manifest']).read_text()) if item['ecosystem'] == 'npm' else tomllib.loads((root / item['manifest']).read_text())
        package = document if item['ecosystem'] == 'npm' else document['package']
        version = package['version']
        if isinstance(version, dict): version = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version']
        if package['name'] != item['name'] or not re.fullmatch(r'[0-9A-Za-z.+-]+', version):
            raise ValueError('Invalid archive identity')
        name = package['name'].replace('@', '').replace('/', '-')
        key = 'PACKAGE_ARCHIVE_' + re.sub(r'[^A-Z0-9]', '_', name.upper()) + '_' + item['ecosystem'].upper()
        if key in result: raise ValueError('Archive output name collision')
        result[key] = name + '-' + version + ('.tgz' if item['ecosystem'] == 'npm' else '.crate')
    return result

if __name__ == '__main__':
    with open(os.environ['GITHUB_ENV'], 'a') as output:
        for key, value in archive_names(Path(os.environ['GITHUB_WORKSPACE'])).items(): output.write(key + '=' + value + '\n')
