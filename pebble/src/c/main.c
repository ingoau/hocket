// App entry: phone messaging, settings, touch dispatch and navigation between the windows.
#include "hocket.h"

Settings g_settings;
NowState g_now;
bool g_version_mismatch;
bool g_phone_seen;

#define PERSIST_SETTINGS 1
#define HOLD_MS 500
#define HOLD_SLOP_PX 12

// -- Outbox ----------------------------------------------------------------------------------------

typedef struct {
  uint8_t type;
  uint8_t cmd;
  int32_t arg;
  uint16_t req_id;
  uint16_t offset;
  char path[96];
  char item[64];
} OutMsg;

#define OUT_QUEUE 8
static OutMsg s_out[OUT_QUEUE];
static int s_out_head, s_out_count;
static bool s_out_busy;

static void prv_pump(void);

static void prv_enqueue(const OutMsg *msg) {
  if (msg->type == MsgListReq || msg->type == MsgArtReq) {
    // Only the newest request of a kind matters: replace a queued one (not the one being sent).
    // List requests only replace one for the same path, so another list's request still goes.
    for (int i = s_out_busy ? 1 : 0; i < s_out_count; i++) {
      OutMsg *m = &s_out[(s_out_head + i) % OUT_QUEUE];
      if (m->type == msg->type && (msg->type != MsgListReq || !strcmp(m->path, msg->path))) {
        *m = *msg;
        prv_pump();
        return;
      }
    }
  }
  if (s_out_count == OUT_QUEUE) {
    // Full: drop the oldest; the phone resends state anyway.
    s_out_head = (s_out_head + 1) % OUT_QUEUE;
    s_out_count--;
  }
  s_out[(s_out_head + s_out_count) % OUT_QUEUE] = *msg;
  s_out_count++;
  prv_pump();
}

static void prv_pump(void) {
#ifdef HOCKET_DEMO
  s_out_count = 0;
  return;
#endif
  if (s_out_busy || s_out_count == 0) {
    return;
  }
  DictionaryIterator *iter;
  if (app_message_outbox_begin(&iter) != APP_MSG_OK) {
    return;
  }
  const OutMsg *m = &s_out[s_out_head];
  dict_write_uint8(iter, MESSAGE_KEY_Type, m->type);
  switch (m->type) {
    case MsgHello:
      dict_write_uint32(iter, MESSAGE_KEY_Proto, HOCKET_PROTO);
      dict_write_cstring(iter, MESSAGE_KEY_Build, HOCKET_WATCH_VERSION);
      break;
    case MsgCmd:
      dict_write_uint8(iter, MESSAGE_KEY_Cmd, m->cmd);
      dict_write_int32(iter, MESSAGE_KEY_Arg, m->arg);
      if (m->path[0]) dict_write_cstring(iter, MESSAGE_KEY_Path, m->path);
      if (m->item[0]) dict_write_cstring(iter, MESSAGE_KEY_Item, m->item);
      break;
    case MsgListReq:
      dict_write_uint16(iter, MESSAGE_KEY_ReqId, m->req_id);
      dict_write_uint16(iter, MESSAGE_KEY_Offset, m->offset);
      dict_write_cstring(iter, MESSAGE_KEY_Path, m->path);
      break;
    case MsgArtReq:
      dict_write_uint16(iter, MESSAGE_KEY_ArtGen, m->req_id);
      dict_write_int32(iter, MESSAGE_KEY_Arg, m->arg);
      break;
  }
  if (app_message_outbox_send() == APP_MSG_OK) {
    s_out_busy = true;
  }
}

static void prv_outbox_sent(DictionaryIterator *iter, void *context) {
  s_out_busy = false;
  if (s_out_count) {
    s_out_head = (s_out_head + 1) % OUT_QUEUE;
    s_out_count--;
  }
  prv_pump();
}

static void prv_outbox_failed(DictionaryIterator *iter, AppMessageResult reason, void *context) {
  // Not connected or the phone is busy: drop it rather than replaying stale presses later.
  prv_outbox_sent(iter, context);
}

void send_cmd(Cmd cmd, int32_t arg, const char *path, const char *item) {
  OutMsg m = {.type = MsgCmd, .cmd = cmd, .arg = arg};
  if (path) strncpy(m.path, path, sizeof(m.path) - 1);
  if (item) strncpy(m.item, item, sizeof(m.item) - 1);
  prv_enqueue(&m);
}

void send_list_request(uint16_t req_id, const char *path, uint16_t offset) {
  OutMsg m = {.type = MsgListReq, .req_id = req_id, .offset = offset};
  strncpy(m.path, path, sizeof(m.path) - 1);
  prv_enqueue(&m);
}

void send_art_request(uint16_t gen, int16_t w, int16_t h) {
  OutMsg m = {.type = MsgArtReq, .req_id = gen, .arg = ((int32_t)w << 16) | (uint16_t)h};
  prv_enqueue(&m);
}

static void prv_send_hello(void) {
  OutMsg m = {.type = MsgHello};
  prv_enqueue(&m);
}

// -- Settings --------------------------------------------------------------------------------------

static const uint8_t DEFAULT_MENU[] = {ItemNowPlaying, ItemQueue, ItemLibrary, ItemLove, ItemRating,
                                       ItemShuffle, ItemRepeat, ItemGoAlbum, ItemGoArtist, ItemPlayOn};
static const uint8_t DEFAULT_ACTIONS[] = {ItemRating, ItemLove, ItemShuffle, ItemRepeat,
                                          ItemGoAlbum, ItemGoArtist, ItemPlayOn};

static void prv_default_settings(Settings *s) {
  *s = (Settings){
    .show_volume_controls = true,
    .show_progress_bar = true,
    .show_album_art = true,
    .gestures = {GestureQueue, GestureNone, GestureLibrary, GestureBack, GestureActions},
    .menu_timeout_s = 5,
    .menu_count = sizeof(DEFAULT_MENU),
    .actions_count = sizeof(DEFAULT_ACTIONS),
  };
  memcpy(s->menu, DEFAULT_MENU, sizeof(DEFAULT_MENU));
  memcpy(s->actions, DEFAULT_ACTIONS, sizeof(DEFAULT_ACTIONS));
}

// Blob: [1 version][flags][5 gestures][timeout][n menu][menu…][n actions][actions…]
static bool prv_parse_settings(const uint8_t *b, size_t len, Settings *out) {
  if (len < 9 || b[0] != 1) return false;
  Settings s;
  prv_default_settings(&s);
  s.touch_navigation = b[1] & 1;
  s.buttons_only = b[1] & 2;
  s.show_volume_controls = b[1] & 4;
  s.show_progress_bar = b[1] & 8;
  s.show_album_art = b[1] & 16;
  for (int i = 0; i < GestureCount; i++) s.gestures[i] = b[2 + i] <= GestureLyrics ? b[2 + i] : GestureNone;
  s.menu_timeout_s = b[7];
  size_t at = 8;
  uint8_t n = b[at++];
  if (n > MAX_MENU_ITEMS || at + n >= len) return false;
  s.menu_count = 0;
  for (int i = 0; i < n; i++) if (b[at + i] < ItemCount) s.menu[s.menu_count++] = b[at + i];
  at += n;
  n = b[at++];
  if (n > MAX_MENU_ITEMS || at + n > len) return false;
  s.actions_count = 0;
  for (int i = 0; i < n; i++) if (b[at + i] < ItemCount) s.actions[s.actions_count++] = b[at + i];
  *out = s;
  return true;
}

static void prv_apply_settings(void) {
#if defined(PBL_TOUCH)
  app_touch_navigation_enable(g_settings.touch_navigation);
#endif
  now_playing_settings_changed();
  list_refresh_local();
}

static void prv_load_settings(void) {
  prv_default_settings(&g_settings);
  uint8_t blob[64];
  int len = persist_read_data(PERSIST_SETTINGS, blob, sizeof(blob));
  if (len > 0) prv_parse_settings(blob, len, &g_settings);
}

// -- Inbox -----------------------------------------------------------------------------------------

static uint32_t prv_u32(DictionaryIterator *iter, uint32_t key, uint32_t fallback) {
  Tuple *t = dict_find(iter, key);
  if (!t) return fallback;
  switch (t->length) {
    case 1: return t->value->uint8;
    case 2: return t->value->uint16;
    default: return t->value->uint32;
  }
}

static void prv_str(DictionaryIterator *iter, uint32_t key, char *dst, size_t n) {
  Tuple *t = dict_find(iter, key);
  if (t && t->type == TUPLE_CSTRING) {
    strncpy(dst, t->value->cstring, n - 1);
    dst[n - 1] = 0;
  } else {
    dst[0] = 0;
  }
}

static void prv_stamp_position(uint32_t position_ms) {
  g_now.position_ms = position_ms;
  time_ms(&g_now.position_at, &g_now.position_at_ms);
}

uint32_t now_position_ms(void) {
  if (!(g_now.flags & NowPlaying)) return g_now.position_ms;
  time_t s;
  uint16_t ms;
  time_ms(&s, &ms);
  int64_t elapsed = (int64_t)(s - g_now.position_at) * 1000 + ms - g_now.position_at_ms;
  if (elapsed < 0) elapsed = 0;
  uint64_t pos = g_now.position_ms + elapsed;
  if (g_now.duration_ms && pos > g_now.duration_ms) pos = g_now.duration_ms;
  return (uint32_t)pos;
}

static void prv_handle_now(DictionaryIterator *iter) {
  char title[TEXT_LEN], artist[TEXT_LEN];
  prv_str(iter, MESSAGE_KEY_Title, title, sizeof(title));
  prv_str(iter, MESSAGE_KEY_Artist, artist, sizeof(artist));
  const bool changed = strcmp(title, g_now.title) || strcmp(artist, g_now.artist);
  strcpy(g_now.title, title);
  strcpy(g_now.artist, artist);
  prv_str(iter, MESSAGE_KEY_Album, g_now.album, sizeof(g_now.album));
  prv_str(iter, MESSAGE_KEY_ResumeLabel, g_now.resume_label, sizeof(g_now.resume_label));
  g_now.duration_ms = prv_u32(iter, MESSAGE_KEY_Duration, 0);
  prv_stamp_position(prv_u32(iter, MESSAGE_KEY_Position, 0));
  g_now.flags = prv_u32(iter, MESSAGE_KEY_Flags, 0);
  g_now.repeat = prv_u32(iter, MESSAGE_KEY_Repeat, 0);
  g_now.rating = prv_u32(iter, MESSAGE_KEY_Rating, 0);
  g_now.volume = prv_u32(iter, MESSAGE_KEY_Volume, 255);
  g_now.art_gen = prv_u32(iter, MESSAGE_KEY_ArtGen, 0);
  now_playing_update(changed);
  if (changed) lyrics_track_changed();
  list_refresh_local();
}

// Album art arrives as a 4-bit palettised bitmap in chunks, rows packed at (w + 1) / 2 bytes.
#if defined(PBL_PLATFORM_EMERY) || defined(PBL_PLATFORM_GABBRO)
#define ART_SUPPORTED 1
static GBitmap *s_art;
static uint16_t s_art_gen;
static uint32_t s_art_size;
static int16_t s_art_w;

static void prv_handle_art(DictionaryIterator *iter) {
  const uint16_t gen = prv_u32(iter, MESSAGE_KEY_ArtGen, 0);
  if (gen != g_now.art_gen) return;
  now_playing_art_progress();
  const uint32_t offset = prv_u32(iter, MESSAGE_KEY_ArtOffset, 0);
  Tuple *data = dict_find(iter, MESSAGE_KEY_ArtData);
  if (offset == 0) {
    if (s_art) gbitmap_destroy(s_art);
    s_art = NULL;
    Tuple *pal = dict_find(iter, MESSAGE_KEY_ArtPalette);
    const int16_t w = prv_u32(iter, MESSAGE_KEY_ArtWidth, 0);
    const int16_t h = prv_u32(iter, MESSAGE_KEY_ArtHeight, 0);
    if (!pal || pal->length < 16 || w <= 0 || h <= 0) return;
    GColor *palette = malloc(16 * sizeof(GColor));
    if (!palette) return;
    memcpy(palette, pal->value->data, 16);
    s_art = gbitmap_create_blank_with_palette(GSize(w, h), GBitmapFormat4BitPalette, palette, true);
    if (!s_art) {
      free(palette);
      return;
    }
    s_art_gen = gen;
    s_art_w = w;
    s_art_size = prv_u32(iter, MESSAGE_KEY_ArtSize, 0);
  }
  if (!s_art || s_art_gen != gen || !data) return;
  uint8_t *dst = gbitmap_get_data(s_art);
  const uint16_t bpr = gbitmap_get_bytes_per_row(s_art);
  const uint32_t stride = (s_art_w + 1) / 2;
  const int16_t rows = gbitmap_get_bounds(s_art).size.h;
  for (uint32_t i = 0; i < data->length; i++) {
    const uint32_t g = offset + i;
    const uint32_t row = g / stride;
    if (row >= (uint32_t)rows) break;
    dst[row * bpr + g % stride] = data->value->data[i];
  }
  if (offset + data->length >= s_art_size) {
    GBitmap *done = s_art;
    s_art = NULL;
    now_playing_art_ready(done);
  }
}
#else
#define ART_SUPPORTED 0
#endif

static void prv_inbox(DictionaryIterator *iter, void *context) {
  g_phone_seen = true;
  switch (prv_u32(iter, MESSAGE_KEY_Type, 0)) {
    case MsgNow:
      prv_handle_now(iter);
      break;
    case MsgList:
      // Each takes only the pages it asked for (by ReqId), wherever it is in the stack.
      lyrics_handle_page(iter);
      list_handle_page(iter);
      break;
    case MsgArt:
#if ART_SUPPORTED
      prv_handle_art(iter);
#endif
      break;
    case MsgSettings: {
      Tuple *t = dict_find(iter, MESSAGE_KEY_Settings);
      if (t && prv_parse_settings(t->value->data, t->length, &g_settings)) {
        persist_write_data(PERSIST_SETTINGS, t->value->data, t->length < 64 ? t->length : 64);
        prv_apply_settings();
      }
      break;
    }
    case MsgHelloReply:
      g_version_mismatch = prv_u32(iter, MESSAGE_KEY_Proto, HOCKET_PROTO) != HOCKET_PROTO;
      now_playing_update(false);
      break;
    case MsgNotice: {
      char text[64];
      prv_str(iter, MESSAGE_KEY_Notice, text, sizeof(text));
      if (text[0]) show_notice(text);
      break;
    }
  }
}

static void prv_connection(bool connected) {
  if (connected) prv_send_hello();
}

// -- Touch -----------------------------------------------------------------------------------------

static HoldHandler s_hold_handler;
static void *s_hold_ctx;
#if defined(PBL_TOUCH)
static AppTimer *s_hold_timer;
static int16_t s_down_x, s_down_y;
#endif

void touch_set_hold_handler(HoldHandler handler, void *ctx) {
  s_hold_handler = handler;
  s_hold_ctx = ctx;
}

void touch_clear_hold_handler(void *ctx) {
  if (s_hold_ctx == ctx) touch_set_hold_handler(NULL, NULL);
}

bool has_touch(void) {
#if defined(PBL_TOUCH)
  return touch_service_is_enabled();
#else
  return false;
#endif
}

bool touch_mode(void) {
  return has_touch() && !g_settings.buttons_only;
}

#if defined(PBL_TOUCH)
static void prv_hold_fired(void *context) {
  s_hold_timer = NULL;
  if (s_hold_handler && touch_mode()) {
    vibes_short_pulse();
    s_hold_handler(s_hold_ctx);
  }
}

static void prv_touch(const TouchEvent *event, void *context) {
  switch (event->type) {
    case TouchEvent_Touchdown:
      s_down_x = event->x;
      s_down_y = event->y;
      if (s_hold_timer) app_timer_cancel(s_hold_timer);
      s_hold_timer = app_timer_register(HOLD_MS, prv_hold_fired, NULL);
      break;
    case TouchEvent_PositionUpdate:
      if (s_hold_timer && (abs(event->x - s_down_x) > HOLD_SLOP_PX || abs(event->y - s_down_y) > HOLD_SLOP_PX)) {
        app_timer_cancel(s_hold_timer);
        s_hold_timer = NULL;
      }
      break;
    case TouchEvent_Liftoff:
      if (s_hold_timer) {
        app_timer_cancel(s_hold_timer);
        s_hold_timer = NULL;
      }
      break;
  }
}
#endif

// -- Navigation ------------------------------------------------------------------------------------

void pop_to_now_playing(void) {
  Window *np = now_playing_window();
  while (window_stack_get_top_window() != np && window_stack_get_top_window()) {
    window_stack_pop(window_stack_contains_window(np) ? true : false);
  }
}

void open_gesture(Gesture g) {
  switch (g) {
    case GestureQueue: list_push_remote("queue", "Queue"); break;
    case GestureLibrary: list_push_remote("library", "Library"); break;
    case GestureActions: list_push_menu(ListActions); break;
    case GestureBack: window_stack_pop(true); break;
    case GestureLyrics: lyrics_push(); break;
    case GestureNone: break;
  }
}

void open_menu_item(MenuItemId id, bool from_menu) {
  switch (id) {
    case ItemNowPlaying: pop_to_now_playing(); break;
    case ItemQueue: list_push_remote("queue", "Queue"); break;
    case ItemLibrary: list_push_remote("library", "Library"); break;
    case ItemPlaylists: list_push_remote("playlists", "Playlists"); break;
    case ItemAlbums: list_push_remote("albums", "Albums"); break;
    case ItemArtists: list_push_remote("artists", "Artists"); break;
    case ItemDownloads: list_push_remote("downloads", "Downloads"); break;
    case ItemSearch: list_start_search(); break;
    case ItemLyrics: lyrics_push(); break;
    case ItemRating: list_push_rating(); break;
    case ItemGoAlbum: list_push_remote("goAlbum", g_now.album[0] ? g_now.album : "Album"); break;
    case ItemGoArtist: list_push_remote("goArtist", g_now.artist[0] ? g_now.artist : "Artist"); break;
    case ItemPlayOn: list_push_remote("devices", "Play on"); break;
    case ItemLove:
      send_cmd(CmdToggleLove, 0, NULL, NULL);
      g_now.flags ^= NowLoved;  // optimistic; the phone's next state confirms
      pop_to_now_playing();
      break;
    case ItemShuffle:
      send_cmd(CmdToggleShuffle, 0, NULL, NULL);
      g_now.flags ^= NowShuffle;
      pop_to_now_playing();
      break;
    case ItemRepeat:
      send_cmd(CmdCycleRepeat, 0, NULL, NULL);
      g_now.repeat = (g_now.repeat + 1) % 3;
      pop_to_now_playing();
      break;
    case ItemCount: break;
  }
}

const char *menu_item_label(MenuItemId id, char *buf, size_t n) {
  static const char *const REPEAT[] = {"Off", "All", "One"};
  switch (id) {
    case ItemNowPlaying: return "Now Playing";
    case ItemQueue: return "Queue";
    case ItemLibrary: return "Library";
    case ItemLove: return (g_now.flags & NowLoved) ? "Loved" : "Love";
    case ItemRating:
      if (g_now.rating) {
        snprintf(buf, n, "Rating: %d of 5", g_now.rating);
        return buf;
      }
      return "Rate";
    case ItemShuffle: return (g_now.flags & NowShuffle) ? "Shuffle: On" : "Shuffle: Off";
    case ItemRepeat:
      snprintf(buf, n, "Repeat: %s", REPEAT[g_now.repeat % 3]);
      return buf;
    case ItemGoAlbum: return "Go to album";
    case ItemGoArtist: return "Go to artist";
    case ItemPlayOn: return "Play on…";
    case ItemPlaylists: return "Playlists";
    case ItemAlbums: return "Albums";
    case ItemArtists: return "Artists";
    case ItemDownloads: return "Downloads";
    case ItemSearch: return "Search";
    case ItemLyrics: return "Lyrics";
    case ItemCount: break;
  }
  return "";
}

// -- Notice: a short message over everything, gone after a moment ---------------------------------

static Window *s_notice_window;
static TextLayer *s_notice_text;
static char s_notice_buf[64];

static void prv_notice_timeout(void *context) {
  if (s_notice_window && window_stack_contains_window(s_notice_window)) {
    window_stack_remove(s_notice_window, true);
  }
}

static void prv_notice_unload(Window *window) {
  text_layer_destroy(s_notice_text);
  window_destroy(s_notice_window);
  s_notice_window = NULL;
}

void show_notice(const char *text) {
  strncpy(s_notice_buf, text, sizeof(s_notice_buf) - 1);
  // Nothing to hold on the notice; the window below sets its handler again when it reappears.
  touch_set_hold_handler(NULL, NULL);
  if (!s_notice_window) {
    s_notice_window = window_create();
    window_set_background_color(s_notice_window, PBL_IF_COLOR_ELSE(HOCKET_DARK, GColorBlack));
    window_set_window_handlers(s_notice_window, (WindowHandlers){.unload = prv_notice_unload});
    Layer *root = window_get_root_layer(s_notice_window);
    GRect b = layer_get_bounds(root);
    s_notice_text = text_layer_create(GRect(10, b.size.h / 2 - 30, b.size.w - 20, 60));
    text_layer_set_background_color(s_notice_text, GColorClear);
    text_layer_set_text_color(s_notice_text, GColorWhite);
    text_layer_set_font(s_notice_text, fonts_get_system_font(FONT_KEY_GOTHIC_24_BOLD));
    text_layer_set_text_alignment(s_notice_text, GTextAlignmentCenter);
    layer_add_child(root, text_layer_get_layer(s_notice_text));
    window_stack_push(s_notice_window, true);
  }
  text_layer_set_text(s_notice_text, s_notice_buf);
  app_timer_register(1500, prv_notice_timeout, NULL);
}

// -- App -------------------------------------------------------------------------------------------

#ifdef HOCKET_DEMO
static void prv_demo(void) {
  strcpy(g_now.title, "Signal Fires");
  strcpy(g_now.artist, "The Example Band");
  strcpy(g_now.album, "Placeholder Sessions");
  g_now.duration_ms = 330000;
  prv_stamp_position(97000);
  g_now.flags = NowPlaying | NowHasTrack | NowReportsPosition | NowLoved;
  g_now.rating = 4;
  g_phone_seen = true;
  now_playing_update(true);
}
#endif

static void prv_init(void) {
  prv_load_settings();
  app_message_register_inbox_received(prv_inbox);
  app_message_register_outbox_sent(prv_outbox_sent);
  app_message_register_outbox_failed(prv_outbox_failed);
  // Big enough for a page of list rows or a chunk of art.
  app_message_open(4096, 512);
  connection_service_subscribe((ConnectionHandlers){.pebble_app_connection_handler = prv_connection});
#if defined(PBL_TOUCH)
  if (touch_service_is_enabled()) touch_service_subscribe(prv_touch, NULL);
#endif
  now_playing_push();
  prv_apply_settings();
  prv_send_hello();
#ifdef HOCKET_DEMO
  prv_demo();
#endif
}

static void prv_deinit(void) {
#if defined(PBL_TOUCH)
  touch_service_unsubscribe();
#endif
  connection_service_unsubscribe();
  app_message_deregister_callbacks();
}

int main(void) {
  prv_init();
  app_event_loop();
  prv_deinit();
}
