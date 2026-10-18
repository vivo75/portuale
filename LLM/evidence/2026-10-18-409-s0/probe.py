import sys
sys.path.insert(0,'/usr/lib/python3.14/site-packages')
from portage.util import getconfig
d=getconfig(sys.argv[1], expand=True)
for k in sorted(d): print(k, repr(d[k]))
