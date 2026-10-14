"""Repository-owned release contract: version tags select one public package."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import urllib.request

SEMVER = r'(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?'

def select_package(root, contract, ecosystem, tag):
    candidates = []
    for item in contract['packages']:
        if item['ecosystem'] != ecosystem:
            continue
        path = Path(item['manifest'])
        if path.is_absolute() or '..' in path.parts:
            raise ValueError('Manifest must remain inside its owning repository')
        resolved = (root / path).resolve()
        if not resolved.is_relative_to(root.resolve()) or (root / path).is_symlink():
            raise ValueError('Manifest must be an owned regular file')
        raw = resolved.read_text()
        document = json.loads(raw) if ecosystem == 'npm' else tomllib.loads(raw)
        package = document if ecosystem == 'npm' else document['package']
        if ecosystem == 'npm' and package.get('private') is True:
            raise ValueError('Private package cannot be published')
        if ecosystem == 'cargo' and package.get('publish') != ['crates-io']:
            raise ValueError('Cargo publication must explicitly target crates-io')
        if package['name'] != item['name']:
            raise ValueError('Package identity changed without a contract update')
        version = package.get('version')
        if isinstance(version, dict) and version.get('workspace') is True:
            version = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version']
        if not isinstance(version, str) or not re.fullmatch(SEMVER, version):
            raise ValueError('A concrete package SemVer is required')
        slug = package['name'].replace('@', '').replace('/', '--')
        if tag == f'{ecosystem}/{slug}/v{version}' or (len(contract['packages']) == 1 and tag == 'v' + version):
            candidates.append({**item, 'version': version, 'directory': str(path.parent)})
    if len(candidates) != 1:
        raise ValueError('Tag must select exactly one package and match its manifest version')
    return candidates[0]

def verify_source(root, contract, environment):
    if environment['GITHUB_REPOSITORY'] != contract['repository']:
        raise ValueError('Repository identity mismatch')
    if environment['GITHUB_REF_TYPE'] != 'tag':
        raise ValueError('Release requires a version tag')
    sha = environment['GITHUB_SHA']
    if not re.fullmatch('[a-f0-9]{40}', sha):
        raise ValueError('A full immutable source SHA is required')
    event = json.loads(Path(environment['GITHUB_EVENT_PATH']).read_text())
    if event['repository']['private'] or event['repository']['full_name'] != contract['repository']:
        raise ValueError('Only this public repository can publish')
    if subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip() != sha:
        raise ValueError('Checkout SHA differs from event SHA')
    subprocess.run(['git', 'merge-base', '--is-ancestor', sha, 'origin/' + contract['defaultBranch']], cwd=root, check=True)
    if subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], cwd=root).strip():
        raise ValueError('Tracked source changed before release admission')
    workflows = contract['requiredWorkflows']
    if not workflows:
        raise ValueError('A maintained CI workflow must be configured')
    url = f"https://api.github.com/repos/{contract['repository']}/actions/runs?head_sha={sha}&per_page=100"
    request = urllib.request.Request(url, headers={'Authorization': 'Bearer ' + environment['GH_TOKEN'], 'Accept': 'application/vnd.github+json', 'User-Agent': 'repository-release-admission'})
    with urllib.request.urlopen(request, timeout=30) as response:
        runs = json.load(response)['workflow_runs']
    for workflow in workflows:
        matches = [r for r in runs if r.get('path', '').split('@')[0] == '.github/workflows/' + workflow
                   and r.get('event') == 'push' and r.get('head_branch') == contract['defaultBranch']]
        if not matches or matches[0]['status'] != 'completed' or matches[0]['conclusion'] != 'success':
            raise ValueError('Required default-branch CI has not succeeded: ' + workflow)

def main():
    root = Path.cwd()
    contract = json.loads((root / '.github/registry-release.json').read_text())
    item = select_package(root, contract, sys.argv[1], os.environ['GITHUB_REF_NAME'])
    verify_source(root, contract, os.environ)
    for key, value in {'name': item['name'], 'version': item['version'], 'directory': item['directory'],
                       'install-directory': item.get('installDirectory', '.'), 'manifest': item['manifest']}.items():
        if '\n' in value or '\r' in value:
            raise ValueError('Invalid workflow output')
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write(f'{key}={value}\n')
    print(json.dumps({'source': os.environ['GITHUB_SHA'], 'tag': os.environ['GITHUB_REF_NAME'], 'package': item['name'], 'version': item['version']}))

if __name__ == '__main__':
    main()
