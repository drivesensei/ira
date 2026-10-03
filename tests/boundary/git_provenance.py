"""Read-only candidate Git checks; never trust hidden index source entries."""
import hashlib
import os
import subprocess


def git_at(root,*args):
    return subprocess.check_output(["git",*args],cwd=root,text=True,env={**os.environ,"GIT_OPTIONAL_LOCKS":"0"}).strip()


def index_flags(root,scope=None):
    args=["ls-files","-v","-z","--"]
    if scope:args.append(scope)
    # Lowercase status means assume-unchanged; S/s means skip-worktree.
    return {record[2:] for record in git_at(root,*args).split("\0") if record and (record[0].islower() or record[0]=="S")}


def tree(root,revision,scope=None):
    args=["ls-tree","-r","-z",revision,"--"]
    if scope:args.append(scope)
    result={}
    for record in git_at(root,*args).split("\0"):
        if record:
            metadata,name=record.split("\t",1);mode,_,blob=metadata.split();result[name]=(mode,blob)
    return result


def actual_differs(path,mode,blob):
    if path.is_symlink():data=os.fsencode(os.readlink(path));actual_mode="120000"
    elif path.is_file():data=path.read_bytes();actual_mode="100755" if path.stat().st_mode&0o100 else "100644"
    else:return True
    digest=hashlib.sha1(b"blob "+str(len(data)).encode()+b"\0"+data).hexdigest()
    return actual_mode!=mode or digest!=blob


def candidate_errors(root,expected_head):
    errors=[]
    if git_at(root,"rev-parse","HEAD")!=expected_head:errors.append("candidate HEAD mismatch")
    # Actual tracked bytes/types also catch misleading cached stat metadata.
    if git_at(root,"status","--porcelain","--untracked-files=all") or any(actual_differs(root/name,*record) for name,record in tree(root,"HEAD").items()):errors.append("dirty candidate")
    if index_flags(root):errors.append("candidate index flags")
    return errors


def changed_root_paths(root):
    # Ignore cached stat metadata/config; inspect actual oracle source bytes.
    baseline=tree(root,"tui-oracle-baseline","src")
    actual={str(p.relative_to(root)):p for p in (root/"src").rglob("*") if p.is_symlink() or not p.is_dir()}
    changed=set(actual)^set(baseline)
    changed.update(name for name in set(actual)&set(baseline) if actual_differs(actual[name],*baseline[name]))
    return changed | index_flags(root,"src")
