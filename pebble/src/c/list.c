// Every list: phone lists paged in on demand (queue, library, albums, search results, devices…)
// and the watch's own menus (the buttons-only menu, the actions menu, the rating picker and the
// Play next / Add to queue menu a held row opens).
#include "hocket.h"

#define CACHE 24
#define PAGE 12
#define REQUEST_TIMEOUT_MS 5000

typedef struct {
  uint8_t flags;
  char id[48];
  char title[48];
  char sub[40];
} Row;

typedef struct List List;
struct List {
  List *next;  // the open lists
  Window *window;
  MenuLayer *menu;
  Layer *touch_shield;  // see prv_load; NULL with touch navigation on
  ListKind kind;
  char path[96];
  char title[48];
  // Remote
  uint16_t req_id;
  bool loading;
  AppTimer *req_timer;  // gives up on a request the phone has not answered
  bool have_total;
  uint16_t total;
  uint16_t base;
  uint8_t count;
  Row rows[CACHE];
  // Local
  uint8_t n_local;
  uint8_t local[MAX_MENU_ITEMS];
  AppTimer *timeout;
  // Hold menu target
  char hold_path[96];
  char hold_item[64];
};

static uint16_t s_next_req = 1;
static List *s_lists;

static List *prv_top(void) {
  Window *w = window_stack_get_top_window();
  if (!w || w == now_playing_window()) return NULL;
  return window_get_user_data(w);
}

static bool prv_is_list_window(Window *w);

// -- Remote paging ---------------------------------------------------------------------------------

#ifdef HOCKET_DEMO
// Sample rows so the emulator shows lists without a phone.
static void prv_demo_page(List *l, uint16_t offset) {
  static const char *const LIBRARY[][2] = {
    {"playlists", "Playlists"}, {"albums", "Albums"}, {"artists", "Artists"},
    {"recent", "Recently played"}, {"downloads", "Downloads"}, {"search", "Search"},
  };
  static const char *const QUEUE[][2] = {
    {"Track One", "The Example Band"}, {"Track Two", "The Example Band"}, {"Signal Fires", "The Example Band"},
    {"Track Four", "The Example Band"}, {"Track Five", "The Example Band"}, {"Track Six", "The Example Band"},
    {"Track Seven", "The Example Band"}, {"Track Eight", "The Example Band"},
  };
  l->base = 0;
  l->have_total = true;
  if (!strcmp(l->path, "library")) {
    l->count = l->total = ARRAY_LENGTH(LIBRARY);
    for (int i = 0; i < l->count; i++) {
      l->rows[i] = (Row){.flags = !strcmp(LIBRARY[i][0], "search") ? RowSearch : RowBrowsable};
      strcpy(l->rows[i].id, LIBRARY[i][0]);
      strcpy(l->rows[i].title, LIBRARY[i][1]);
    }
  } else if (!strcmp(l->path, "queue")) {
    l->count = l->total = ARRAY_LENGTH(QUEUE);
    for (int i = 0; i < l->count; i++) {
      l->rows[i] = (Row){.flags = RowTrack | (i == 2 ? RowCurrent : 0)};
      snprintf(l->rows[i].id, sizeof(l->rows[i].id), "%d", i);
      strcpy(l->rows[i].title, QUEUE[i][0]);
      strcpy(l->rows[i].sub, i == 2 ? "Now playing" : QUEUE[i][1]);
    }
  } else {
    l->count = l->total = 2 + 6;
    l->rows[0] = (Row){.id = "play", .title = "Play"};
    l->rows[1] = (Row){.id = "shuffle", .title = "Shuffle"};
    for (int i = 2; i < l->count; i++) {
      l->rows[i] = (Row){.flags = RowTrack};
      snprintf(l->rows[i].id, sizeof(l->rows[i].id), "t:%d", i - 2);
      strcpy(l->rows[i].title, QUEUE[i - 2][0]);
      strcpy(l->rows[i].sub, "4:25");
    }
  }
  menu_layer_reload_data(l->menu);
  for (int i = 0; i < l->count; i++) {
    if (l->rows[i].flags & RowCurrent) menu_layer_set_selected_index(l->menu, MenuIndex(0, i), MenuRowAlignCenter, false);
  }
  (void)offset;
}
#endif

static void prv_request(List *l, uint16_t offset);

// No reply (the send failed, or the phone's did): stop waiting so the rows ask again.
static void prv_request_timeout(void *ctx) {
  List *l = ctx;
  l->req_timer = NULL;
  l->loading = false;
  if (!l->have_total) prv_request(l, 0);
  else menu_layer_reload_data(l->menu);
}

static void prv_request(List *l, uint16_t offset) {
  if (l->loading) return;
  l->loading = true;
  l->req_id = s_next_req++;
  if (s_next_req >= 0x8000) s_next_req = 1;  // lyrics use the ids above
  l->req_timer = app_timer_register(REQUEST_TIMEOUT_MS, prv_request_timeout, l);
#ifdef HOCKET_DEMO
  l->loading = false;
  app_timer_cancel(l->req_timer);
  l->req_timer = NULL;
  prv_demo_page(l, offset);
#else
  send_list_request(l->req_id, l->path, offset);
#endif
}

static bool prv_cached(List *l, uint16_t index) {
  return index >= l->base && index < l->base + l->count;
}

static void prv_ensure(List *l, uint16_t index) {
  if (prv_cached(l, index) || l->loading) return;
  uint16_t offset = index >= l->base + l->count ? index : (index >= PAGE - 1 ? index - (PAGE - 1) : 0);
  prv_request(l, offset);
}

// Rows: [flags][id\0][title\0][sub\0]…
void list_handle_page(DictionaryIterator *iter) {
  Tuple *req = dict_find(iter, MESSAGE_KEY_ReqId);
  if (!req) return;
  List *l = s_lists;
  while (l && !(l->kind == ListRemote && l->loading && l->req_id == req->value->uint16)) l = l->next;
  if (!l) return;
  l->loading = false;
  if (l->req_timer) app_timer_cancel(l->req_timer);
  l->req_timer = NULL;
  Tuple *t = dict_find(iter, MESSAGE_KEY_Total);
  Tuple *o = dict_find(iter, MESSAGE_KEY_Offset);
  Tuple *items = dict_find(iter, MESSAGE_KEY_Items);
  Tuple *title = dict_find(iter, MESSAGE_KEY_ListTitle);
  if (title && title->type == TUPLE_CSTRING && title->value->cstring[0]) {
    strncpy(l->title, title->value->cstring, sizeof(l->title) - 1);
  }
  l->total = t ? t->value->uint16 : 0;
  l->have_total = true;
  const uint16_t offset = o ? o->value->uint16 : 0;
  Row page[PAGE];
  int n = 0;
  if (items) {
    const uint8_t *p = items->value->data, *end = p + items->length;
    while (p < end && n < PAGE) {
      Row *r = &page[n];
      r->flags = *p++;
      char *fields[] = {r->id, r->title, r->sub};
      const size_t sizes[] = {sizeof(r->id), sizeof(r->title), sizeof(r->sub)};
      for (int f = 0; f < 3; f++) {
        const uint8_t *z = memchr(p, 0, end - p);
        if (!z) { p = end; break; }
        size_t len = z - p;
        if (len >= sizes[f]) len = sizes[f] - 1;
        memcpy(fields[f], p, len);
        fields[f][len] = 0;
        p = z + 1;
      }
      n++;
    }
  }
  // Merge with what is cached when the page is adjacent, otherwise replace.
  if (l->count && offset == l->base + l->count) {
    int drop = l->count + n - CACHE;
    if (drop > 0) {
      memmove(l->rows, l->rows + drop, (l->count - drop) * sizeof(Row));
      l->base += drop;
      l->count -= drop;
    }
    memcpy(l->rows + l->count, page, n * sizeof(Row));
    l->count += n;
  } else if (l->count && offset + n == l->base) {
    int keep = l->count;
    if (keep + n > CACHE) keep = CACHE - n;
    memmove(l->rows + n, l->rows, keep * sizeof(Row));
    memcpy(l->rows, page, n * sizeof(Row));
    l->base = offset;
    l->count = keep + n;
  } else {
    memcpy(l->rows, page, n * sizeof(Row));
    l->base = offset;
    l->count = n;
  }
  const bool first = offset == 0 && menu_layer_get_selected_index(l->menu).row == 0;
  menu_layer_reload_data(l->menu);
  // Open the queue on the playing row.
  if (first) {
    for (int i = 0; i < l->count; i++) {
      if (l->rows[i].flags & RowCurrent) {
        menu_layer_set_selected_index(l->menu, MenuIndex(0, l->base + i), MenuRowAlignCenter, false);
        break;
      }
    }
  }
}

// -- Local menus -----------------------------------------------------------------------------------

static const char *prv_local_label(List *l, uint16_t row, char *buf, size_t n) {
  switch (l->kind) {
    case ListMenu:
    case ListActions:
      return menu_item_label(l->local[row], buf, n);
    case ListRating:
      if (row == 0) return "No rating";
      snprintf(buf, n, row == 1 ? "%d star" : "%d stars", row);
      return buf;
    case ListHold:
      return row == 0 ? "Play next" : "Add to queue";
    case ListRemote:
      break;
  }
  return "";
}

// -- MenuLayer -------------------------------------------------------------------------------------

static void prv_reset_timeout(List *l);

static uint16_t prv_rows(MenuLayer *menu, uint16_t section, void *ctx) {
  List *l = ctx;
  if (l->kind != ListRemote) return l->n_local;
  if (!l->have_total) return 1;
  return l->total ? l->total : 1;
}

static int16_t prv_header_height(MenuLayer *menu, uint16_t section, void *ctx) {
  return PBL_IF_RECT_ELSE(MENU_CELL_BASIC_HEADER_HEIGHT, 0);
}

static void prv_draw_header(GContext *ctx, const Layer *cell, uint16_t section, void *data) {
  List *l = data;
  menu_cell_basic_header_draw(ctx, cell, l->title);
}

static void prv_draw_row(GContext *ctx, const Layer *cell, MenuIndex *index, void *data) {
  List *l = data;
  char buf[32];
  if (l->kind != ListRemote) {
    const char *sub = NULL;
    if (l->kind == ListRating && index->row == g_now.rating) sub = "Current";
    menu_cell_basic_draw(ctx, cell, prv_local_label(l, index->row, buf, sizeof(buf)), sub, NULL);
    return;
  }
  if (!l->have_total) {
    menu_cell_basic_draw(ctx, cell, "Loading…", NULL, NULL);
    return;
  }
  if (!l->total) {
    menu_cell_basic_draw(ctx, cell, "Nothing here", NULL, NULL);
    return;
  }
  if (!prv_cached(l, index->row)) {
    prv_ensure(l, index->row);
    menu_cell_basic_draw(ctx, cell, "…", NULL, NULL);
    return;
  }
  Row *r = &l->rows[index->row - l->base];
  if (r->flags & RowHeader) {
    menu_cell_basic_header_draw(ctx, cell, r->title);
    return;
  }
  menu_cell_basic_draw(ctx, cell, r->title, r->sub[0] ? r->sub : NULL, NULL);
}

static int16_t prv_row_height(MenuLayer *menu, MenuIndex *index, void *ctx) {
  List *l = ctx;
  if (l->kind == ListRemote && prv_cached(l, index->row)) {
    Row *r = &l->rows[index->row - l->base];
    if (r->flags & RowHeader) return MENU_CELL_BASIC_HEADER_HEIGHT + 4;
    if (!r->sub[0]) return PBL_IF_ROUND_ELSE(MENU_CELL_ROUND_UNFOCUSED_SHORT_CELL_HEIGHT, 44);
  }
  return PBL_IF_ROUND_ELSE(menu_layer_is_index_selected(menu, index) ? MENU_CELL_ROUND_FOCUSED_TALL_CELL_HEIGHT
                                                                     : MENU_CELL_ROUND_UNFOCUSED_TALL_CELL_HEIGHT, 56);
}

static void prv_selection_changed(MenuLayer *menu, MenuIndex new_index, MenuIndex old_index, void *ctx) {
  List *l = ctx;
  prv_reset_timeout(l);
  if (l->kind == ListRemote && l->have_total) {
    // Prefetch before the edge of what is cached.
    if (new_index.row + 3 >= l->base + l->count && l->base + l->count < l->total) prv_ensure(l, l->base + l->count);
    else if (new_index.row < l->base + 3 && l->base > 0) prv_ensure(l, l->base - 1);
  }
}

static void prv_hold_row(List *l, uint16_t row) {
  if (l->kind != ListRemote || !prv_cached(l, row)) return;
  Row *r = &l->rows[row - l->base];
  if (r->flags & (RowHeader | RowSearch)) return;
  if (!(r->flags & RowTrack) && strncmp(r->id, "album:", 6) && strncmp(r->id, "playlist:", 9)) return;
  list_push_hold(l->path, r->id, r->title);
}

static void prv_select(MenuLayer *menu, MenuIndex *index, void *ctx) {
  List *l = ctx;
  prv_reset_timeout(l);
  switch (l->kind) {
    case ListMenu:
    case ListActions:
      if (l->timeout) app_timer_cancel(l->timeout);
      l->timeout = NULL;
      open_menu_item(l->local[index->row], l->kind == ListMenu);
      return;
    case ListRating:
      send_cmd(CmdSetRating, index->row, NULL, NULL);
      g_now.rating = index->row;
      pop_to_now_playing();
      return;
    case ListHold:
      send_cmd(index->row == 0 ? CmdPlayNext : CmdPlayLater, 0, l->hold_path, l->hold_item);
      show_notice(index->row == 0 ? "Playing next" : "Added to queue");
      window_stack_remove(l->window, false);
      return;
    case ListRemote:
      break;
  }
  if (!prv_cached(l, index->row)) return;
  Row *r = &l->rows[index->row - l->base];
  if (r->flags & RowHeader) return;
  if (r->flags & RowSearch) {
    list_start_search();
  } else if (r->flags & RowBrowsable) {
    list_push_remote(r->id, r->title);
  } else {
    send_cmd(CmdActivate, 0, l->path, r->id);
    pop_to_now_playing();
  }
}

static void prv_long_select(MenuLayer *menu, MenuIndex *index, void *ctx) {
  prv_hold_row(ctx, index->row);
}

static void prv_touch_hold(void *ctx) {
  List *l = ctx;
  prv_hold_row(l, menu_layer_get_selected_index(l->menu).row);
}

// -- Timeout (buttons-only menu) and back --------------------------------------------------------

static void prv_timeout_fired(void *ctx) {
  List *l = ctx;
  l->timeout = NULL;
  if (window_stack_get_top_window() == l->window) window_stack_pop(true);
}

static void prv_reset_timeout(List *l) {
  if (l->kind != ListMenu || !g_settings.menu_timeout_s) return;
  if (window_stack_get_top_window() != l->window) return;
  const uint32_t ms = g_settings.menu_timeout_s * 1000;
  if (l->timeout) app_timer_reschedule(l->timeout, ms);
  else l->timeout = app_timer_register(ms, prv_timeout_fired, l);
}

static ClickConfigProvider s_menu_click_provider;

static void prv_menu_back(ClickRecognizerRef recognizer, void *ctx) {
  // Back from the buttons-only menu leaves the app.
  window_stack_pop_all(true);
}

static void prv_menu_click_config(void *ctx) {
  if (s_menu_click_provider) s_menu_click_provider(ctx);
  window_single_click_subscribe(BUTTON_ID_BACK, prv_menu_back);
}

#if defined(PBL_TOUCH)
static void prv_swipe(const Recognizer *recognizer, RecognizerEvent event) {
  if (event == RecognizerEvent_Completed && touch_mode() &&
      swipe_recognizer_get_direction(recognizer) == SwipeDirection_Right) {
    window_stack_pop(true);
  }
}
#endif

// -- Window ----------------------------------------------------------------------------------------

static void prv_load(Window *window) {
  List *l = window_get_user_data(window);
  Layer *root = window_get_root_layer(window);
  GRect bounds = layer_get_bounds(root);
  l->menu = menu_layer_create(bounds);
  menu_layer_set_callbacks(l->menu, l, (MenuLayerCallbacks){
    .get_num_rows = prv_rows,
    .get_header_height = prv_header_height,
    .draw_header = prv_draw_header,
    .draw_row = prv_draw_row,
    .get_cell_height = prv_row_height,
    .select_click = prv_select,
    .select_long_click = prv_long_select,
    .selection_changed = prv_selection_changed,
  });
#ifdef PBL_COLOR
  menu_layer_set_highlight_colors(l->menu, PBL_IF_COLOR_ELSE(HOCKET_ACCENT, GColorRed), GColorWhite);
#endif
  menu_layer_set_click_config_onto_window(l->menu, window);
  if (l->kind == ListMenu) {
    s_menu_click_provider = window_get_click_config_provider(window);
    window_set_click_config_provider_with_context(window, prv_menu_click_config, l->menu);
  }
  layer_add_child(root, menu_layer_get_layer(l->menu));
#if defined(PBL_TOUCH)
  if (!g_settings.touch_navigation) {
    // Touch only does gestures: swipe right goes back, press and hold acts on the selected row.
    window_set_touch_bridge_disabled(window, true);
    window_attach_recognizer(window, swipe_recognizer_create(prv_swipe, NULL, SwipeDirection_Right));
    // The app is always opted into touch navigation (main.c), and an opted-in app's MenuLayer
    // scrolls and selects by touch on its own: the OS hands a touch to it when the layer under the
    // finger is the menu or inside it. An empty layer over the whole menu is what the finger lands
    // on instead, so the list ignores touch. The window's recognizers and the press-and-hold (raw
    // touch) don't depend on that layer and keep working.
    l->touch_shield = layer_create(bounds);
    layer_add_child(root, l->touch_shield);
  }
#endif
  if (l->kind == ListRemote) prv_request(l, 0);
}

static void prv_appear(Window *window) {
  List *l = window_get_user_data(window);
  touch_set_hold_handler(prv_touch_hold, l);
  if (l->kind != ListRemote) menu_layer_reload_data(l->menu);
  prv_reset_timeout(l);
}

static void prv_disappear(Window *window) {
  List *l = window_get_user_data(window);
  if (l->timeout) app_timer_cancel(l->timeout);
  l->timeout = NULL;
  touch_clear_hold_handler(l);
}

static void prv_unload(Window *window) {
  List *l = window_get_user_data(window);
  touch_clear_hold_handler(l);
  if (l->timeout) app_timer_cancel(l->timeout);
  if (l->req_timer) app_timer_cancel(l->req_timer);
  for (List **p = &s_lists; *p; p = &(*p)->next) {
    if (*p == l) {
      *p = l->next;
      break;
    }
  }
  if (l->touch_shield) layer_destroy(l->touch_shield);
  menu_layer_destroy(l->menu);
  window_destroy(window);
  free(l);
}

static bool prv_is_list_window(Window *w) {
  return w && w != now_playing_window();
}

static List *prv_push(ListKind kind, const char *title) {
  List *l = calloc(1, sizeof(List));
  if (!l) {
    show_notice("Out of memory");
    return NULL;
  }
  l->kind = kind;
  l->next = s_lists;
  s_lists = l;
  strncpy(l->title, title, sizeof(l->title) - 1);
  l->window = window_create();
  window_set_user_data(l->window, l);
  window_set_window_handlers(l->window, (WindowHandlers){
    .load = prv_load, .appear = prv_appear, .disappear = prv_disappear, .unload = prv_unload,
  });
  return l;
}

void list_push_remote(const char *path, const char *title) {
  List *l = prv_push(ListRemote, title);
  if (!l) return;
  strncpy(l->path, path, sizeof(l->path) - 1);
  window_stack_push(l->window, true);
}

void list_push_menu(ListKind kind) {
  List *l = prv_push(kind, kind == ListMenu ? "Hocket" : "Actions");
  if (!l) return;
  const uint8_t *items = kind == ListMenu ? g_settings.menu : g_settings.actions;
  const uint8_t n = kind == ListMenu ? g_settings.menu_count : g_settings.actions_count;
  for (int i = 0; i < n; i++) l->local[l->n_local++] = items[i];
  window_stack_push(l->window, true);
}

void list_push_rating(void) {
  List *l = prv_push(ListRating, "Rating");
  if (!l) return;
  l->n_local = 6;
  window_stack_push(l->window, true);
  menu_layer_set_selected_index(l->menu, MenuIndex(0, g_now.rating), MenuRowAlignCenter, false);
}

void list_push_hold(const char *path, const char *item, const char *title) {
  List *l = prv_push(ListHold, title);
  if (!l) return;
  l->n_local = 2;
  strncpy(l->hold_path, path, sizeof(l->hold_path) - 1);
  strncpy(l->hold_item, item, sizeof(l->hold_item) - 1);
  window_stack_push(l->window, true);
}

void list_refresh_local(void) {
  List *l = prv_top();
  if (l && prv_is_list_window(l->window) && l->kind != ListRemote) menu_layer_reload_data(l->menu);
}

// -- Voice search ------------------------------------------------------------------------------------

#if defined(PBL_MICROPHONE)
static DictationSession *s_dictation;
static char s_search_path[96];

static void prv_dictation_done(DictationSession *session, DictationSessionStatus status, char *text, void *ctx) {
  if (status == DictationSessionStatusSuccess && text && text[0]) {
    snprintf(s_search_path, sizeof(s_search_path), "search:%s", text);
    list_push_remote(s_search_path, text);
  }
}
#endif

void list_start_search(void) {
#if defined(PBL_MICROPHONE)
  if (!s_dictation) s_dictation = dictation_session_create(sizeof(s_search_path) - 8, prv_dictation_done, NULL);
  if (s_dictation) {
    dictation_session_enable_confirmation(s_dictation, false);
    dictation_session_start(s_dictation);
    return;
  }
#endif
  show_notice("Search needs a microphone");
}
