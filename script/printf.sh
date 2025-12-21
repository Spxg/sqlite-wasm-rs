TAG=6.3.0
curl -L https://github.com/eyalroz/printf/archive/refs/tags/v$TAG.zip > $TAG.zip
unzip -p "$TAG.zip" "printf-$TAG/src/printf/printf.c" > "shim/printf/printf.c"
unzip -p "$TAG.zip" "printf-$TAG/src/printf/printf.h" > "shim/printf/printf.h"
rm $TAG.zip
