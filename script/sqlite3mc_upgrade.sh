SQLITE=sqlite3mc-2.2.7-sqlite-3.51.2-amalgamation
curl -L https://github.com/utelle/SQLite3MultipleCiphers/releases/latest/download/$SQLITE.zip > $SQLITE.zip
unzip -p "$SQLITE.zip" "sqlite3mc_amalgamation.c" > "sqlite3mc/sqlite3mc_amalgamation.c"
unzip -p "$SQLITE.zip" "sqlite3mc_amalgamation.h" > "sqlite3mc/sqlite3mc_amalgamation.h"
rm -f "$SQLITE.zip"
