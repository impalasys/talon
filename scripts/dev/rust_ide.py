#!/usr/bin/env python3
"""Opt-in, machine-local Bazel project model for Zed. Cargo never compiles code."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

STATE = '.rules_rust_analyzer'
IGNORE = '\n# Talon Bazel Rust IDE (machine-local)\n/.rules_rust_analyzer/\n/rust-project.json\n/.zed/settings.json\n'
PROVIDERS = '''def format(target):
    result = []
    for key, value in providers(target).items():
        if key.endswith("%CrateInfo") or key.endswith("%TestCrateInfo"):
            crate = getattr(value, "crate", value)
            result.append({"root": crate.root.path, "source": crate.root.is_source, "env_files": [f.path for f in crate.rustc_env_files]})
    return json.encode(result)
'''


def run(command, workspace, capture=False):
    return subprocess.run(command, cwd=workspace, check=True, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout


def atomic_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile('w', dir=path.parent, delete=False) as f:
        json.dump(value, f, indent=2)
        f.write('\n')
        temporary = Path(f.name)
    temporary.replace(path)


def protect_generated(workspace):
    exclude = Path(run(['git', 'rev-parse', '--git-path', 'info/exclude'], workspace, True).strip())
    if not exclude.is_absolute():
        exclude = workspace / exclude
    existing = exclude.read_text() if exclude.exists() else ''
    if '# Talon Bazel Rust IDE (machine-local)' not in existing:
        exclude.parent.mkdir(parents=True, exist_ok=True)
        exclude.write_text(existing + IGNORE)
    for relative in [STATE + '/probe', 'rust-project.json', '.zed/settings.json']:
        if subprocess.run(['git', 'check-ignore', '-q', relative], cwd=workspace).returncode:
            raise RuntimeError(f'{relative} must be ignored before generating IDE state')


def parse_env(path):
    result = {}
    for line in path.read_text().splitlines():
        if not line:
            continue
        name, separator, value = line.partition('=')
        if not separator:
            raise ValueError(f'Unsupported environment-file record in {path}')
        result[name] = value
    return result


def package_scope(workspace):
    # Avoid walking ignored, nested editor worktrees with a recursive //... query.
    files = run(['git', 'ls-files', '-co', '--exclude-standard', '-z', '--',
                 'BUILD', 'BUILD.bazel', '**/BUILD', '**/BUILD.bazel'], workspace, True)
    packages = sorted({'//' + (str(Path(p).parent) if Path(p).parent != Path('.') else '') + ':all'
                       for p in files.split('\0') if p})
    if not packages:
        raise RuntimeError('No Git-visible Bazel packages found')
    return 'set(' + ' '.join(json.dumps(p) for p in packages) + ')'


def sysroot_model(metadata):
    # Supplying this graph prevents rust-analyzer from automatically invoking
    # Cargo even for the standard library. Only explicit setup uses metadata;
    # it never runs Cargo build/check or any standard-library build scripts.
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    libraries = []
    for package in metadata['packages']:
        target = next((t for t in package['targets'] if
                       set(t['kind']) & {'lib', 'rlib', 'dylib', 'proc-macro'}), None)
        if target:
            libraries.append((package, target))
    indices = {p['id']: index for index, (p, _) in enumerate(libraries)}
    crates = []
    for package, target in libraries:
        node = nodes[package['id']]
        crates.append({
            'display_name': target['name'], 'root_module': target['src_path'],
            'edition': package['edition'], 'is_workspace_member': False,
            'cfg': ['bootstrap'] + [f'feature="{f}"' for f in node['features']],
            'env': {'CARGO_PKG_VERSION': package['version'],
                    'CARGO_MANIFEST_DIR': str(Path(package['manifest_path']).parent)},
            'deps': [{'crate': indices[d['pkg']], 'name': d['name']} for d in node['deps']
                     if d['pkg'] in indices and any(k['kind'] is None for k in d['dep_kinds'])],
        })
    names = {c['display_name'] for c in crates}
    if not {'std', 'core', 'alloc', 'test', 'proc_macro'} <= names:
        raise RuntimeError('Incomplete standard-library source graph')
    return {'crates': crates}


def add_sysroot(project, workspace):
    state = workspace / STATE / 'sysroot-project.json'
    root = project['sysroot_src']
    saved = json.loads(state.read_text()) if state.exists() else {}
    if saved.get('source') != root:
        rustc = str(Path(project['sysroot']) / 'bin/rustc')
        host = subprocess.check_output([rustc, '-vV'], text=True).split('host: ')[1].splitlines()[0]
        env = dict(os.environ, RUSTC=rustc, RUSTC_WRAPPER='', RUSTC_WORKSPACE_WRAPPER='', RUSTC_BOOTSTRAP='1')
        print('Reading standard-library Cargo metadata once (no compilation)', flush=True)
        metadata = subprocess.check_output(['cargo', 'metadata', '--format-version=1', '--locked',
                                           '--filter-platform', host, '--manifest-path', root + '/Cargo.toml'],
                                          cwd=workspace, env=env, text=True)
        saved = {'source': root, 'project': sysroot_model(json.loads(metadata))}
        atomic_json(state, saved)
    project['sysroot_project'] = saved['project']


def finish_model(project, records, workspace, execution_root):
    overrides = {}
    for record in records:
        base = execution_root if not record['source'] or record['root'].startswith('external/') else workspace
        root = str(base / record['root'])
        values = overrides.setdefault(root, {})
        for relative in record['env_files']:
            values.update(parse_env(execution_root / relative))
    for crate in project['crates']:
        crate.setdefault('env', {}).update(overrides.get(crate['root_module'], {}))
    # Compiler checks stay explicit in this pilot. In particular do not invoke
    # upstream flycheck's second output_base/JSON-diagnostics configuration.
    project['runnables'] = [r for r in project.get('runnables', []) if r['kind'].lower() != 'flycheck']
    return project


def zed_settings(existing, workspace, tools, log_file=None):
    result = json.loads(json.dumps(existing))
    lsp = result.setdefault('lsp', {}).setdefault('rust-analyzer', {})
    lsp.setdefault('binary', {}).update(
        path=tools['rust-analyzer.server.path'],
        arguments=['--log-file', str(log_file or workspace / STATE / 'zed-rust-analyzer.log')])
    init = lsp.setdefault('initialization_options', {})
    # The upstream generator writes its root file before postprocessing. Link a
    # separate, atomically published model so live refreshes cannot expose a
    # temporary model without sysroot_project (and thereby trigger Cargo).
    init['linkedProjects'] = [str(workspace / STATE / 'rust-project.json')]
    init['checkOnSave'] = False
    cargo = init.setdefault('cargo', {})
    cargo['autoreload'] = False
    cargo.setdefault('buildScripts', {}).update(enable=False, rebuildOnSave=False)
    init.setdefault('procMacro', {}).update(enable=True, server=tools['rust-analyzer.procMacro.server'])
    init.setdefault('rustfmt', {})['overrideCommand'] = tools['rust-analyzer.rustfmt.overrideCommand']
    # A prior Cargo/Bazel discovery override must not add another project graph.
    init.setdefault('workspace', {})['discoverConfig'] = None
    return result


def refresh(workspace, bazel, config, setup=False):
    protect_generated(workspace)
    state = workspace / STATE
    state.mkdir(exist_ok=True)
    tools_path = state / 'tools.json'
    version_path = state / 'toolchain-inputs.txt'
    version = hashlib.sha256((workspace / 'MODULE.bazel').read_bytes() +
                             (workspace / '.bazelversion').read_bytes()).hexdigest()
    tools = json.loads(tools_path.read_text()) if tools_path.exists() else {}
    paths = [tools.get(key, '') for key in ['rust-analyzer.server.path', 'rust-analyzer.procMacro.server']]
    paths.append(next(iter(tools.get('rust-analyzer.rustfmt.overrideCommand', [])), ''))
    if (setup or not all(Path(path).is_file() for path in paths) or
            not version_path.exists() or version_path.read_text() != version):
        text = run([bazel, 'run', '@rules_rust//tools/rust_analyzer:setup', '--', '--no-per-package-workspaces', 'print'], workspace, True)
        atomic_json(tools_path, json.loads(text))
        version_path.write_text(version)
    scope = package_scope(workspace)
    labels = run([bazel, 'query', f'kind("rust_(library|binary|test) rule", {scope})', '--output=label'], workspace, True).splitlines()
    if not labels:
        raise RuntimeError('Bazel returned no Rust targets')
    flags = [f'--config={config}'] if config else []
    run([bazel, 'run', '@rules_rust//tools/rust_analyzer:gen_rust_project', '--',
         '--bazel', str(Path(shutil.which(bazel) or bazel).resolve()), *flags, *labels], workspace)
    info = run([bazel, 'info', 'execution_root'], workspace, True).strip()
    execution_root = Path(info)
    query = state / 'env_files.cquery'
    query.write_text(PROVIDERS)
    text = run([bazel, 'cquery', 'set(' + ' '.join(labels) + ')', *flags,
                '--output=starlark', '--starlark:file=' + str(query)], workspace, True)
    records = [row for line in text.splitlines() if line for row in json.loads(line)]
    # These actions generate only the env files; they do not compile the app.
    env_files = sorted({relative for r in records for relative in r['env_files']})
    targets = run([bazel, 'query', f'kind("cargo_toml_env_vars rule", {scope})', '--output=label'], workspace, True).splitlines()
    if targets:
        run([bazel, 'build', *flags, *targets], workspace)
    missing = [p for p in env_files if not (execution_root / p).is_file()]
    if missing:
        raise RuntimeError('Environment files not materialized: ' + ', '.join(missing))
    path = workspace / 'rust-project.json'
    project = finish_model(json.loads(path.read_text()), records, workspace, execution_root)
    add_sysroot(project, workspace)
    atomic_json(path, project)
    atomic_json(state / 'rust-project.json', project)
    atomic_json(state / 'project-state.json', {'workspace': str(workspace), 'config': config, 'roots': labels,
                'crates': len(project['crates']), 'proc_macros': sum(c.get('is_proc_macro', False) for c in project['crates'])})
    return json.loads(tools_path.read_text())


def restore(workspace):
    settings = workspace / '.zed/settings.json'
    backup = workspace / STATE / 'zed-settings-backup.json'
    installed = workspace / STATE / 'installed-settings.json'
    if installed.exists() and (not settings.exists() or settings.read_text() != installed.read_text()):
        raise RuntimeError(f'Project settings changed since setup; merge the backup at {backup} manually')
    original = json.loads(backup.read_text())
    if original['existed']:
        settings.write_text(original['text'])
    else:
        settings.unlink(missing_ok=True)
    model = workspace / 'rust-project.json'
    if original.get('model') is not None:
        model.write_text(original['model'])
    else:
        model.unlink(missing_ok=True)
    (workspace / STATE / 'rust-project.json').unlink(missing_ok=True)
    backup.unlink()
    installed.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['setup', 'refresh', 'restore'])
    parser.add_argument('--workspace', type=Path, help='Defaults to the current Git worktree root')
    parser.add_argument('--bazel', default='bazel')
    parser.add_argument('--config', help='One Bazel configuration; define a combined config in .bazelrc.local if needed')
    args = parser.parse_args()
    workspace = (args.workspace or Path(run(['git', 'rev-parse', '--show-toplevel'], Path.cwd(), True).strip())).resolve()
    settings = workspace / '.zed/settings.json'
    backup = workspace / STATE / 'zed-settings-backup.json'
    if args.command == 'restore':
        restore(workspace)
        print('Restored original Talon Zed project settings')
        return
    if args.command == 'refresh' and not backup.exists():
        raise RuntimeError('Run setup before refresh')
    # Refuse an unsupported settings format before doing any generation.
    existing_text = settings.read_text() if settings.exists() else None
    existing = json.loads(existing_text) if existing_text is not None else {}
    if args.command == 'setup' and not backup.exists():
        protect_generated(workspace)
        model = workspace / 'rust-project.json'
        atomic_json(backup, {'existed': existing_text is not None, 'text': existing_text,
                             'model': model.read_text() if model.exists() else None})
    config = args.config
    state = workspace / STATE / 'project-state.json'
    if args.command == 'refresh' and config is None and state.exists():
        config = json.loads(state.read_text()).get('config')
    tools = refresh(workspace, args.bazel, config, args.command == 'setup')
    # cargo.autoreload=false also disables automatic graph reloads. Changing the
    # log argument makes Zed restart this server after each explicit refresh,
    # loading the newly published graph without an extra editor command.
    log_file = workspace / STATE / 'logs' / f'{time.time_ns()}.log'
    log_file.parent.mkdir(exist_ok=True)
    latest = workspace / STATE / 'zed-rust-analyzer.log'
    latest.unlink(missing_ok=True)
    latest.symlink_to(Path('logs') / log_file.name)
    configured = zed_settings(existing, workspace, tools, log_file)
    atomic_json(settings, configured)
    atomic_json(workspace / STATE / 'installed-settings.json', configured)
    local_helper = workspace / STATE / 'rust_ide.py'
    if Path(__file__).resolve() != local_helper.resolve():
        shutil.copyfile(__file__, local_helper)
    print('Installed Talon-only Bazel-backed Zed settings; compiler checks are explicit Bazel commands')


if __name__ == '__main__':
    main()
