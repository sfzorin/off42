#!/bin/zsh
# compose_all.sh OUTDIR LANG… — one video and poster per language (3 in parallel).
V=${0:A:h}; OUT=$1; shift
mkdir -p $OUT
compose_one() {
  local l=$1
  cd $V && python3 compose3.py $l $OUT/off42-demo-$l.mp4 > $V/tmp/compose-$l.log 2>&1 && \
  python3 compose3.py $l x --preview 3.2 >> $V/tmp/compose-$l.log 2>&1 && \
  python3 -c "from PIL import Image; Image.open('$V/tmp/pv-$l/preview-3.2.png').convert('RGB').save('$OUT/off42-demo-$l.jpg', quality=85, optimize=True, progressive=True)" && \
  echo "OK $l $(stat -f %z $OUT/off42-demo-$l.mp4)" || echo "FAIL $l"
}
for l in $@; do
  while (( $(jobs -r | wc -l) >= 3 )); do sleep 2; done
  compose_one $l &
done
wait
