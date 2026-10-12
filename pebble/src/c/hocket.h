// The Hocket watchapp: shared protocol, state and window entry points. docs/pebble.md is the spec.
#pragma once

#include <pebble.h>

#ifndef HOCKET_WATCH_VERSION
#define HOCKET_WATCH_VERSION "dev"
#endif

// Bumped on any incompatible change to the messages below. The phone checks it on Hello.
#define HOCKET_PROTO 1

// -- Message types (MESSAGE_KEY_Type) ------------------------------------------------------------

// Phone -> watch
enum {
  MsgNow = 1,       // now-playing state
  MsgList = 2,      // one page of a list
  MsgArt = 3,       // one chunk of the album art
  MsgSettings = 4,  // the settings blob
  MsgHelloReply = 5,// Proto / Build of the phone, after a Hello
  MsgNotice = 6,    // a short message to flash (Notice)
};

// Watch -> phone
enum {
  MsgHello = 10,    // Proto, Build
  MsgCmd = 11,      // Cmd, Arg, Path, Item
  MsgListReq = 12,  // ReqId, Path, Offset
  MsgArtReq = 13,   // ArtGen, Arg = width << 16 | height
};

// Now-playing flags (MESSAGE_KEY_Flags)
enum {
  NowPlaying = 1 << 0,
  NowShuffle = 1 << 1,
  NowLoved = 1 << 2,
  NowHasTrack = 1 << 3,
  NowCanResume = 1 << 4,
  NowHasVolume = 1 << 5,
  NowReportsPosition = 1 << 6,
};

// Commands (MESSAGE_KEY_Cmd)
typedef enum {
  CmdPlayPause = 1,
  CmdPlay = 2,
  CmdPause = 3,
  CmdNext = 4,
  CmdPrevious = 5,
  CmdVolumeUp = 6,
  CmdVolumeDown = 7,
  CmdToggleLove = 8,
  CmdSetRating = 9,   // Arg = 0..5
  CmdToggleShuffle = 10,
  CmdCycleRepeat = 11,
  CmdResume = 12,
  CmdActivate = 13,   // Path, Item: play / open the row
  CmdPlayNext = 14,   // Path, Item
  CmdPlayLater = 15,  // Path, Item
} Cmd;

// List row flags (first byte of each packed row)
enum {
  RowBrowsable = 1 << 0,  // selecting opens a list at the row's id
  RowCurrent = 1 << 1,    // the playing item (queue)
  RowSearch = 1 << 2,     // starts dictation, then opens "search:<text>"
  RowTrack = 1 << 3,      // a track: holding offers Play next / Add to queue
  RowHeader = 1 << 4,     // not selectable
};

// -- Settings ------------------------------------------------------------------------------------

typedef enum {
  GestureNone = 0,
  GestureQueue = 1,
  GestureLibrary = 2,
  GestureActions = 3,
  GestureBack = 4,
} Gesture;

typedef enum {
  GestureUp, GestureDown, GestureLeft, GestureRight, GestureHold, GestureCount,
} GestureSlot;

// Ids of rows in the buttons-only menu and the actions menu (the phone sends them in this numbering).
typedef enum {
  ItemNowPlaying = 0,
  ItemQueue = 1,
  ItemLibrary = 2,
  ItemLove = 3,
  ItemRating = 4,
  ItemShuffle = 5,
  ItemRepeat = 6,
  ItemGoAlbum = 7,
  ItemGoArtist = 8,
  ItemPlayOn = 9,
  ItemPlaylists = 10,
  ItemAlbums = 11,
  ItemArtists = 12,
  ItemDownloads = 13,
  ItemSearch = 14,
  ItemCount,
} MenuItemId;

#define MAX_MENU_ITEMS 16

typedef struct {
  bool touch_navigation;
  bool buttons_only;
  bool show_volume_controls;
  bool show_progress_bar;
  bool show_album_art;
  uint8_t gestures[GestureCount];
  uint8_t menu_timeout_s;
  uint8_t menu_count;
  uint8_t menu[MAX_MENU_ITEMS];
  uint8_t actions_count;
  uint8_t actions[MAX_MENU_ITEMS];
} Settings;

// -- Now-playing state -----------------------------------------------------------------------------

#define TEXT_LEN 96

typedef struct {
  char title[TEXT_LEN];
  char artist[TEXT_LEN];
  char album[TEXT_LEN];
  char resume_label[TEXT_LEN];
  uint32_t duration_ms;
  uint32_t position_ms;
  time_t position_at;     // when position_ms was true (watch clock, seconds)
  uint16_t position_at_ms;
  uint8_t flags;
  uint8_t repeat;         // 0 off, 1 all, 2 one
  uint8_t rating;         // 0..5
  uint8_t volume;         // 0..100, 255 = unknown
  uint16_t art_gen;       // changes when the cover changes; 0 = none
} NowState;

extern Settings g_settings;
extern NowState g_now;
// Set when the phone and watch protocols differ: the app shows "update" instead of half-working.
extern bool g_version_mismatch;
extern bool g_phone_seen;

// -- main.c --------------------------------------------------------------------------------------

void send_cmd(Cmd cmd, int32_t arg, const char *path, const char *item);
void send_list_request(uint16_t req_id, const char *path, uint16_t offset);
void send_art_request(uint16_t gen, int16_t w, int16_t h);
bool has_touch(void);
bool touch_mode(void);           // touch gestures on (a touch watch, not buttons-only)
uint32_t now_position_ms(void);  // interpolated
void open_gesture(Gesture g);    // from Now Playing
void open_menu_item(MenuItemId id, bool from_menu);
const char *menu_item_label(MenuItemId id, char *buf, size_t n);
void pop_to_now_playing(void);
void show_notice(const char *text);

// Touch: each window registers a hold handler; a hold on the screen calls the top window's.
typedef void (*HoldHandler)(void *ctx);
void touch_set_hold_handler(HoldHandler handler, void *ctx);

// -- now_playing.c -------------------------------------------------------------------------------

void now_playing_push(void);
void now_playing_update(bool track_changed);
void now_playing_settings_changed(void);
void now_playing_art_ready(GBitmap *art);  // takes ownership; NULL clears
Window *now_playing_window(void);

// -- list.c --------------------------------------------------------------------------------------

typedef enum { ListRemote, ListMenu, ListActions, ListRating, ListHold } ListKind;

// A list at a phone path: "queue", "library", "album:<id>", "search:<text>", "devices", …
void list_push_remote(const char *path, const char *title);
// The buttons-only menu (with its timeout) or the actions menu.
void list_push_menu(ListKind kind);
void list_push_rating(void);
void list_push_hold(const char *path, const char *item, const char *title);
// Incoming page for the top remote list.
void list_handle_page(DictionaryIterator *iter);
// Refresh labels of open local menus (state changed).
void list_refresh_local(void);
void list_start_search(void);
