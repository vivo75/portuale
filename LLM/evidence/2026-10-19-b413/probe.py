"""Build the package.bashrc fixture under a root and run `ebuild <f> compile`
with the PM given as argv[1] ('portage' or a portuale ebuild binary); print marks."""
import os, subprocess, sys, shutil, pathlib
pm, out = sys.argv[1], pathlib.Path(sys.argv[2])
shutil.rmtree(out, ignore_errors=True)
root, marks = out/"cfg", out/"marks"
repo = root/"repo"; profile = repo/"profiles/base"
(repo/"metadata").mkdir(parents=True); profile.mkdir(parents=True)
(repo/"profiles/repo_name").write_text("bashrcrepo\n")
(repo/"metadata/layout.conf").write_text("masters =\nthin-manifests = true\n" + ("profile-formats = profile-bashrcs\n" if os.environ.get("PB_FORMATS") else ""))
(profile/"eapi").write_text("8\n")
(profile/"make.defaults").write_text('ARCH="amd64"\nACCEPT_KEYWORDS="amd64"\nIUSE_IMPLICIT="prefix"\n')
pkg = repo/"dev-util/bashrcprobe"; pkg.mkdir(parents=True)
eb = pkg/"bashrcprobe-1.ebuild"
eb.write_text('EAPI=8\nDESCRIPTION="p"\nSLOT="0"\nKEYWORDS="amd64"\nS="${WORKDIR}"\n')
etc = root/"etc/portage"; (etc/"repos.conf").mkdir(parents=True)
(etc/"repos.conf/r.conf").write_text(f"[DEFAULT]\nmain-repo = bashrcrepo\n[bashrcrepo]\nlocation = {repo}\n")
(etc/"make.profile").symlink_to(profile)
(etc/"make.conf").write_text('ACCEPT_KEYWORDS="amd64"\nFEATURES="-sandbox -usersandbox -network-sandbox -pid-sandbox -ipc-sandbox"\n')
up = etc/"profile"; (up/"bashrc").mkdir(parents=True)
# written most specific first: Portage orders by specificity, not file order
(up/"package.bashrc").write_text("=dev-util/bashrcprobe-1 exact\ndev-util/bashrcprobe slot\ndev-util/* wild\nsys-apps/other nomatch\n")
for n in ("exact","slot","wild","nomatch"):
    (up/"bashrc"/n).write_text(f'echo "{n} ${{EBUILD_PHASE:-none}}" >> {marks}\n')
(up/"profile.bashrc").write_text(f'echo "userprofile ${{EBUILD_PHASE:-none}}" >> {marks}\n')
(profile/"profile.bashrc").write_text(f'echo "profile ${{EBUILD_PHASE:-none}}" >> {marks}\n')
# a repo profile node without `profile-bashrcs` in its formats: package.bashrc ignored
(profile/"bashrc").mkdir()
(profile/"package.bashrc").write_text("dev-util/bashrcprobe gated\n")
(profile/"bashrc/gated").write_text(f'echo "gated ${{EBUILD_PHASE:-none}}" >> {marks}\n')
env = dict(os.environ, PORTAGE_CONFIGROOT=str(root), PORTAGE_TMPDIR=str(out/"tmp"), DISTDIR=str(out/"dist"))
(out/"dist").mkdir(); (out/"tmp").mkdir()
cmd = ["ebuild"] if pm == "portage" else [pm]
r = subprocess.run(cmd+[str(eb), "compile"], capture_output=True, text=True, env=env)
print("rc", r.returncode); 
if r.returncode: print(r.stderr[-800:])
print(*[l for l in marks.read_text().splitlines() if "depend" not in l and "compile" in l], sep="\n")
