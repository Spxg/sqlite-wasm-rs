SQLITE=sqlite3mc-2.1.2-sqlite-3.50.0-amalgamation
curl -L https://github.com/utelle/SQLite3MultipleCiphers/releases/latest/download/$SQLITE.zip > $SQLITE.zip
unzip -p "$SQLITE.zip" "sqlite3mc_amalgamation.c" > "sqlite3mc_amalgamation.c"
unzip -p "$SQLITE.zip" "sqlite3mc_amalgamation.h" > "sqlite3mc_amalgamation.h"
rm -f "$SQLITE.zip"
