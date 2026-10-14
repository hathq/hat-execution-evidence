"""Bind the prepared archive bytes to one selected source package."""
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import sys
import tarfile
import tomllib
import urllib.parse
import urllib.request
from release_plan import select_package

def inspect_archive(archive, ecosystem, name, version):
    with tarfile.open(archive, 'r:gz') as tar:
        members = tar.getmembers()
        for member in members:
            path = PurePosixPath(member.name)
            if path.is_absolute() or '..' in path.parts or member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
                raise ValueError('Unsafe archive member')
            if any(part == '.env' or part.startswith('.env.') for part in path.parts):
                raise ValueError('Runtime environment file in release archive')
        manifest = 'package/package.json' if ecosystem == 'npm' else f'{name}-{version}/Cargo.toml'
        stream = tar.extractfile(manifest)
        if stream is None: raise ValueError('Archive manifest is missing')
        raw = stream.read().decode()
        package = json.loads(raw) if ecosystem == 'npm' else tomllib.loads(raw)['package']
        if package.get('name') != name or package.get('version') != version or package.get('private') is True:
            raise ValueError('Archive identity differs from selected source')

def main():
    mode, ecosystem, directory = sys.argv[1:]
    folder = Path(directory)
    contract = json.loads(Path('.github/registry-release.json').read_text())
    package = select_package(Path.cwd(), contract, ecosystem, os.environ['GITHUB_REF_NAME'])
    extension = '.tgz' if ecosystem == 'npm' else '.crate'
    archives = list(folder.glob('*' + extension))
    if len(archives) != 1: raise ValueError('Exactly one selected archive is required')
    archive = archives[0]
    if archive.is_symlink() or not archive.is_file(): raise ValueError('Archive must be a regular file')
    inspect_archive(archive, ecosystem, package['name'], package['version'])
    data = archive.read_bytes()
    digest = hashlib.sha256(data).hexdigest()
    receipt = {'source': os.environ['GITHUB_SHA'], 'repository': contract['repository'], 'tag': os.environ['GITHUB_REF_NAME'],
               'ecosystem': ecosystem, 'name': package['name'], 'version': package['version'], 'filename': archive.name,
               'sha256': digest, 'integrity': 'sha512-' + base64.b64encode(hashlib.sha512(data).digest()).decode()}
    if mode == 'prepare':
        (folder / 'release-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write(f'archive-name={archive.name}\narchive-sha256={digest}\n')
    elif mode in ('verify', 'registry'):
        if json.loads((folder / 'release-receipt.json').read_text()) != receipt or digest != os.environ['EXPECTED_ARCHIVE_SHA256']:
            raise ValueError('Prepared archive source or bytes changed')
        if set(p.name for p in folder.iterdir()) != {archive.name, 'release-receipt.json'}:
            raise ValueError('Unexpected publication artifact files')
        if mode == 'registry':
            url = 'https://registry.npmjs.org/' + urllib.parse.quote(package['name'], safe='') + '/' + package['version'] if ecosystem == 'npm' else 'https://crates.io/api/v1/crates/' + package['name'] + '/' + package['version']
            request = urllib.request.Request(url, headers={'User-Agent': 'repository-release-integrity'})
            with urllib.request.urlopen(request, timeout=30) as response: metadata = json.load(response)
            observed = metadata['dist']['integrity'] if ecosystem == 'npm' else metadata['version']['checksum']
            if observed != (receipt['integrity'] if ecosystem == 'npm' else digest): raise ValueError('Official registry bytes differ from prepared artifact')
    else: raise ValueError('Expected prepare, verify or registry')
    print(json.dumps(receipt))

if __name__ == '__main__': main()
