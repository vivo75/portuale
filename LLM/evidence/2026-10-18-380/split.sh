#!/bin/bash
# re-run the file variant with the streams split: stdout to /cin/s.out, stderr to /cin/s.err
sed -i 's#^\$driver \$E merge 2>&1 .*#$driver $E merge >/cin/s.out 2>/cin/s.err#' /cin/cell.sh
bash /cin/cell.sh "$1" file
