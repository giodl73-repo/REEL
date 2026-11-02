"""Read exact Git blobs in one process; never apply checkout or archive substitutions."""
import io,subprocess

def read_git_blobs(root,commit,paths):
 paths=list(paths)
 assert len(paths)==len(set(paths))
 assert '\n' not in commit and '\r' not in commit
 assert all('\n' not in p and '\r' not in p and p for p in paths)
 request=''.join(commit+':'+p+'\n' for p in paths).encode('utf-8')
 result=subprocess.run(['git','cat-file','--batch'],cwd=root,input=request,capture_output=True,check=True)
 stream=io.BytesIO(result.stdout);blobs={}
 for path in paths:
  header=stream.readline().split()
  assert len(header)==3 and header[1]==b'blob',(path,header)
  size=int(header[2]);data=stream.read(size)
  assert len(data)==size and stream.read(1)==b'\n',path
  blobs[path]=data
 assert stream.read()==b'', 'Unexpected Git batch output'
 return blobs
