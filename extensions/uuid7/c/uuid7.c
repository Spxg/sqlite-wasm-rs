/*
** SQLite extension for UUIDv7.
** Adapted from PostgreSQL implementation.
** Original source:
** <https://github.com/postgres/postgres/blob/master/src/backend/utils/adt/uuid.c>
*/
#include "sqlite3ext.h"
SQLITE_EXTENSION_INIT1
#include <assert.h>
#include <string.h>
#include <time.h>
#include <stdint.h>

/*
** We use a local implementation of isxdigit to avoid a dependency on <ctype.h>.
*/
static int sqlite3Isxdigit(int c){
  return (c>='0' && c<='9') || (c>='a' && c<='f') || (c>='A' && c<='F');
}

/*
** Helper to convert hex to int.
*/
static unsigned char sqlite3UuidHexToInt(int h){
  assert( (h>='0' && h<='9') ||  (h>='a' && h<='f') ||  (h>='A' && h<='F') );
  if( h>='0' && h<='9' ) return h - '0';
  if( h>='a' && h<='f' ) return h - 'a' + 10;
  return h - 'A' + 10;
}

/*
** Convert blob to UUID string.
*/
static void sqlite3UuidBlobToStr(const unsigned char *aBlob, unsigned char *zStr){
  static const char zDigits[] = "0123456789abcdef";
  int i, k;
  unsigned char x;
  k = 0;
  for(i=0, k=0x550; i<16; i++, k=k>>1){
    if( k&1 ){
      zStr[0] = '-';
      zStr++;
    }
    x = aBlob[i];
    zStr[0] = zDigits[x>>4];
    zStr[1] = zDigits[x&0xf];
    zStr += 2;
  }
  *zStr = 0;
}

/* 
** UUIDv7 Generation Logic 
*/

#define NS_PER_S	1000000000LL
#define NS_PER_MS	1000000LL

/*
** UUID version 7 uses 12 bits in "rand_a" to store 1/4096 (or 2^12) fractions of
** sub-millisecond.
*/
#define SUBMS_BITS	12
#define SUBMS_MINIMAL_STEP_NS ((NS_PER_MS / (1 << 12)) + 1)

static int64_t get_real_time_ns_ascending(void)
{
    static int64_t last_time_ns = 0;
    struct timespec ts;
    int64_t now_ns;

    clock_gettime(CLOCK_REALTIME, &ts);
    now_ns = (int64_t)ts.tv_sec * NS_PER_S + ts.tv_nsec;

    /*
     * If the clock moved backwards or stalled, we must advance it to
     * maintain monotonicity as required by UUIDv7.
     */
    if (now_ns <= last_time_ns)
    {
        now_ns = last_time_ns + SUBMS_MINIMAL_STEP_NS;
    }

    last_time_ns = now_ns;
    return now_ns;
}

static void generate_uuidv7(unsigned char *uuid_out)
{
    int64_t current_ns;
    uint64_t unix_ts_ms;
    uint32_t sub_ms;
    
    current_ns = get_real_time_ns_ascending();
    
    unix_ts_ms = current_ns / NS_PER_MS;
    /* Calculate sub-millisecond fraction, scaled to 12 bits (4096) */
    sub_ms = (uint32_t)(((current_ns % NS_PER_MS) * 4096) / NS_PER_MS);

    /* Fill with random data first */
    sqlite3_randomness(16, uuid_out);

    /* 
    ** uuid[0-5]: unix_ts_ms (48 bits)
    ** uuid[6]: ver (4 bits) | rand_a (4 bits from sub_ms)
    ** uuid[7]: rand_a (8 bits from sub_ms)
    ** uuid[8]: var (2 bits) | rand_b (6 bits)
    */

    /* Encode timestamp (big-endian) */
    uuid_out[0] = (unsigned char)(unix_ts_ms >> 40);
    uuid_out[1] = (unsigned char)(unix_ts_ms >> 32);
    uuid_out[2] = (unsigned char)(unix_ts_ms >> 24);
    uuid_out[3] = (unsigned char)(unix_ts_ms >> 16);
    uuid_out[4] = (unsigned char)(unix_ts_ms >> 8);
    uuid_out[5] = (unsigned char)(unix_ts_ms);

    /* Version 7 and top 4 bits of sub_ms */
    uuid_out[6] = 0x70 | ((sub_ms >> 8) & 0x0F);
    
    /* Lower 8 bits of sub_ms */
    uuid_out[7] = (unsigned char)(sub_ms & 0xFF);

    /* Variant 1 (0b10xx) */
    uuid_out[8] = (uuid_out[8] & 0x3F) | 0x80;
}

static void sqlite3Uuid7Func(
  sqlite3_context *context,
  int argc,
  sqlite3_value **argv
){
  unsigned char aBlob[16];
  unsigned char zStr[37];
  (void)argc;
  (void)argv;
  
  generate_uuidv7(aBlob);
  sqlite3UuidBlobToStr(aBlob, zStr);
  sqlite3_result_text(context, (char*)zStr, 36, SQLITE_TRANSIENT);
}

#ifdef _WIN32
__declspec(dllexport)
#endif
int sqlite3_uuid7_init(
  sqlite3 *db,
  char **pzErrMsg,
  const sqlite3_api_routines *pApi
){
  int rc = SQLITE_OK;
  SQLITE_EXTENSION_INIT2(pApi);
  (void)pzErrMsg;
  rc = sqlite3_create_function_v2(db, "uuid7", 0, SQLITE_UTF8|SQLITE_INNOCUOUS, 0,
                               sqlite3Uuid7Func, 0, 0, 0);
  return rc;
}
