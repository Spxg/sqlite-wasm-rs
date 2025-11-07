SQLITE=sqlite-amalgamation-3510000
curl -O https://sqlite.org/2025/$SQLITE.zip
unzip -p "$SQLITE.zip" "$SQLITE/sqlite3.c" > "sqlite3.c"
unzip -p "$SQLITE.zip" "$SQLITE/sqlite3.h" > "sqlite3.h"
unzip -p "$SQLITE.zip" "$SQLITE/sqlite3ext.h" > "sqlite3ext.h"
rm -f "$SQLITE.zip"
