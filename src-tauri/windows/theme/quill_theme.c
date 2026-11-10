// Quill's dark installer theme: an NSIS plugin that paints the stock wizard's
// buttons, check boxes, labels and fields in the Settings window's colors.
// NSIS can recolor text but can't draw buttons, so this subclasses each
// control and paints it. Build with build.cmd; installer.nsi calls
//   quill_theme::Init   once, from .onGUIInit
//   quill_theme::Page   from every page's show callback.

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <commctrl.h>
#include <uxtheme.h>
#include <dwmapi.h>
#include "pluginapi.h"

#define RGBHEX(h) RGB(((h) >> 16) & 0xFF, ((h) >> 8) & 0xFF, (h) & 0xFF)

static const COLORREF BG = RGBHEX(0x1C1C1E);       // page
static const COLORREF SIDE = RGBHEX(0x19191B);     // header strip
static const COLORREF FOOT = RGBHEX(0x242426);     // footer
static const COLORREF FIELD = RGBHEX(0x2B2B2D);    // text fields
static const COLORREF FIELD_EDGE = RGBHEX(0x4A4A4C);
static const COLORREF INK = RGBHEX(0xF2F2F7);
static const COLORREF INK2 = RGBHEX(0xA2A2A6);     // secondary text
static const COLORREF INK_OFF = RGBHEX(0x6B6B6E);  // disabled text
static const COLORREF BLUE = RGBHEX(0x0A84FF);
static const COLORREF BTN = RGBHEX(0x333335);
static const COLORREF BTN_HOVER = RGBHEX(0x3C3C3F);
static const COLORREF BTN_DOWN = RGBHEX(0x2C2C2E);
static const COLORREF BTN_EDGE = RGBHEX(0x5C5C5C);
static const COLORREF BTN_OFF = RGBHEX(0x2A2A2C);
static const COLORREF BTN_OFF_EDGE = RGBHEX(0x3E3E40);
static const COLORREF BOX_EDGE = RGBHEX(0x8E8E93);
static const COLORREF TRACK = RGBHEX(0x3A3A3C);
static const COLORREF LOG = RGBHEX(0x141416);

// Controls on the main window (not inside a page).
#define IDC_HEADER_BG 1034
#define IDC_HEADER_TITLE 1037
#define IDC_HEADER_SUB 1038
#define IDC_LINE_1 1035
#define IDC_LINE_2 1036
#define IDC_BRANDING 1028
#define IDC_BRANDING_SHADOW 1256

enum { SUB_MAIN = 1, SUB_PAGE, SUB_BUTTON, SUB_CHECK, SUB_GROUP, SUB_EDIT, SUB_PROGRESS };

static HINSTANCE g_inst;
// One brush per color, kept for the life of the installer.
#define MAX_BRUSHES 32
static HBRUSH g_brushes[MAX_BRUSHES];
static COLORREF g_brush_colors[MAX_BRUSHES];

static HBRUSH brush(COLORREF c) {
  for (int i = 0; i < MAX_BRUSHES; i++) {
    if (g_brushes[i] && g_brush_colors[i] == c) return g_brushes[i];
    if (!g_brushes[i]) {
      g_brush_colors[i] = c;
      return g_brushes[i] = CreateSolidBrush(c);
    }
  }
  return (HBRUSH)GetStockObject(BLACK_BRUSH);
}

static UINT dpi_of(HWND h) {
  typedef UINT(WINAPI * GetDpiForWindowFn)(HWND);
  static GetDpiForWindowFn fn;
  static BOOL looked;
  if (!looked) {
    fn = (GetDpiForWindowFn)GetProcAddress(GetModuleHandleW(L"user32.dll"), "GetDpiForWindow");
    looked = TRUE;
  }
  UINT d = fn ? fn(h) : 96;
  return d ? d : 96;
}

static int px(HWND h, int n) { return MulDiv(n, dpi_of(h), 96); }

static void fill_round(HDC dc, RECT r, int radius, COLORREF fill, COLORREF edge) {
  HPEN pen = CreatePen(PS_SOLID, 1, edge);
  HGDIOBJ old_pen = SelectObject(dc, pen);
  HGDIOBJ old_brush = SelectObject(dc, brush(fill));
  RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
  SelectObject(dc, old_brush);
  SelectObject(dc, old_pen);
  DeleteObject(pen);
}

// The ground a control sits on: the footer for the main window's own
// controls, the header strip for the header, the page for everything else.
static COLORREF ground_of(HWND control) {
  HWND parent = GetParent(control);
  int id = GetDlgCtrlID(control);
  if (id == IDC_HEADER_BG || id == IDC_HEADER_TITLE || id == IDC_HEADER_SUB) return SIDE;
  if (parent && !GetParent(parent)) return FOOT;
  return BG;
}

static void draw_text(HWND h, HDC dc, RECT r, UINT format) {
  WCHAR text[256];
  GetWindowTextW(h, text, 256);
  HGDIOBJ old_font = SelectObject(dc, (HFONT)SendMessageW(h, WM_GETFONT, 0, 0));
  SetBkMode(dc, TRANSPARENT);
  DrawTextW(dc, text, -1, &r, format | DT_HIDEPREFIX);
  SelectObject(dc, old_font);
}

// Paints into a memory bitmap first, so nothing flickers.
typedef void (*painter)(HWND, HDC, RECT);
static void paint_buffered(HWND h, painter fn) {
  PAINTSTRUCT ps;
  HDC dc = BeginPaint(h, &ps);
  RECT r;
  GetClientRect(h, &r);
  HDC mem = CreateCompatibleDC(dc);
  HBITMAP bmp = CreateCompatibleBitmap(dc, r.right, r.bottom);
  HGDIOBJ old = SelectObject(mem, bmp);
  fn(h, mem, r);
  BitBlt(dc, 0, 0, r.right, r.bottom, mem, 0, 0, SRCCOPY);
  SelectObject(mem, old);
  DeleteObject(bmp);
  DeleteDC(mem);
  EndPaint(h, &ps);
}

static void paint_button(HWND h, HDC dc, RECT r) {
  FillRect(dc, &r, brush(ground_of(h)));
  BOOL enabled = IsWindowEnabled(h);
  LRESULT state = SendMessageW(h, BM_GETSTATE, 0, 0);
  BOOL is_default = (GetWindowLongW(h, GWL_STYLE) & 0xF) == BS_DEFPUSHBUTTON;
  BOOL hover = GetPropW(h, L"QuillHover") != NULL;
  COLORREF fill = !enabled ? BTN_OFF : (state & BST_PUSHED) ? BTN_DOWN : hover ? BTN_HOVER : BTN;
  COLORREF edge = !enabled ? BTN_OFF_EDGE : is_default ? BLUE : BTN_EDGE;
  fill_round(dc, r, px(h, 6), fill, edge);
  if (enabled && is_default) {
    // The default button's outline is two pixels, like the mockup.
    RECT in = {r.left + 1, r.top + 1, r.right - 1, r.bottom - 1};
    fill_round(dc, in, px(h, 5), fill, BLUE);
  }
  SetTextColor(dc, enabled ? INK : INK_OFF);
  draw_text(h, dc, r, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
}

static void paint_check(HWND h, HDC dc, RECT r) {
  FillRect(dc, &r, brush(ground_of(h)));
  LONG style = GetWindowLongW(h, GWL_STYLE) & 0xF;
  BOOL radio = style == BS_RADIOBUTTON || style == BS_AUTORADIOBUTTON;
  BOOL checked = SendMessageW(h, BM_GETCHECK, 0, 0) == BST_CHECKED;
  BOOL enabled = IsWindowEnabled(h);
  // Fit the box inside the row, with a pixel spare on every side.
  int size = min(px(h, 14), r.bottom - 2);
  int top = (r.bottom - size) / 2;
  RECT box = {1, top, 1 + size, top + size};
  if (radio) {
    HPEN pen = CreatePen(PS_SOLID, 1, checked ? BLUE : BOX_EDGE);
    HGDIOBJ op = SelectObject(dc, pen);
    HGDIOBJ ob = SelectObject(dc, brush(checked ? BLUE : FIELD));
    Ellipse(dc, box.left, box.top, box.right, box.bottom);
    if (checked) {
      int in = px(h, 4);
      SelectObject(dc, brush(INK));
      SelectObject(dc, GetStockObject(NULL_PEN));
      Ellipse(dc, box.left + in, box.top + in, box.right - in + 1, box.bottom - in + 1);
    }
    SelectObject(dc, ob);
    SelectObject(dc, op);
    DeleteObject(pen);
  } else {
    fill_round(dc, box, px(h, 5), checked ? BLUE : FIELD, checked ? BLUE : BOX_EDGE);
    if (checked) {
      HPEN pen = CreatePen(PS_SOLID, px(h, 2), INK);
      HGDIOBJ op = SelectObject(dc, pen);
      POINT tick[3] = {
          {box.left + size * 3 / 14, box.top + size * 7 / 14},
          {box.left + size * 6 / 14, box.top + size * 10 / 14},
          {box.left + size * 11 / 14, box.top + size * 4 / 14},
      };
      Polyline(dc, tick, 3);
      SelectObject(dc, op);
      DeleteObject(pen);
    }
  }
  RECT text = {box.right + px(h, 7), 0, r.right, r.bottom};
  SetTextColor(dc, enabled ? INK : INK_OFF);
  draw_text(h, dc, text, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
}

// Frames ("Destination Folder") keep their caption and lose the box.
static void paint_group(HWND h) {
  PAINTSTRUCT ps;
  HDC dc = BeginPaint(h, &ps);
  RECT r;
  GetClientRect(h, &r);
  HGDIOBJ old_font = SelectObject(dc, (HFONT)SendMessageW(h, WM_GETFONT, 0, 0));
  TEXTMETRICW tm;
  GetTextMetricsW(dc, &tm);
  SelectObject(dc, old_font);
  r.bottom = r.top + tm.tmHeight;
  FillRect(dc, &r, brush(ground_of(h)));
  SetTextColor(dc, INK);
  draw_text(h, dc, r, DT_LEFT | DT_TOP | DT_SINGLELINE);
  EndPaint(h, &ps);
}

// A slim rounded bar, centered in the control's height.
static void paint_progress(HWND h, HDC dc, RECT r) {
  FillRect(dc, &r, brush(ground_of(h)));
  PBRANGE range;
  SendMessageW(h, PBM_GETRANGE, FALSE, (LPARAM)&range);
  int pos = (int)SendMessageW(h, PBM_GETPOS, 0, 0);
  int span = range.iHigh - range.iLow;
  int height = px(h, 6);
  RECT track = {r.left, (r.bottom - height) / 2, r.right, (r.bottom - height) / 2 + height};
  fill_round(dc, track, height, TRACK, TRACK);
  if (span > 0 && pos > range.iLow) {
    RECT done = track;
    done.right = track.left + MulDiv(track.right - track.left, pos - range.iLow, span);
    if (done.right - done.left < height) done.right = done.left + height;
    fill_round(dc, done, height, BLUE, BLUE);
  }
}

static LRESULT CALLBACK control_proc(HWND h, UINT msg, WPARAM wp, LPARAM lp, UINT_PTR id, DWORD_PTR kind) {
  switch (msg) {
    case WM_PAINT:
      if (kind == SUB_BUTTON) paint_buffered(h, paint_button);
      else if (kind == SUB_CHECK) paint_buffered(h, paint_check);
      else if (kind == SUB_GROUP) paint_group(h);
      else if (kind == SUB_PROGRESS) paint_buffered(h, paint_progress);
      else break;
      return 0;
    case WM_ERASEBKGND:
      if (kind != SUB_EDIT) return 1;
      break;
    case WM_NCPAINT:
      if (kind == SUB_EDIT) {
        LRESULT res = DefSubclassProc(h, msg, wp, lp);
        HDC dc = GetWindowDC(h);
        RECT r;
        GetWindowRect(h, &r);
        OffsetRect(&r, -r.left, -r.top);
        FrameRect(dc, &r, brush(FIELD_EDGE));
        InflateRect(&r, -1, -1);
        FrameRect(dc, &r, brush(FIELD));
        ReleaseDC(h, dc);
        return res;
      }
      break;
    case WM_MOUSEMOVE:
      if (kind == SUB_BUTTON && !GetPropW(h, L"QuillHover")) {
        TRACKMOUSEEVENT t = {sizeof t, TME_LEAVE, h, 0};
        TrackMouseEvent(&t);
        SetPropW(h, L"QuillHover", (HANDLE)1);
        InvalidateRect(h, NULL, FALSE);
      }
      break;
    case WM_MOUSELEAVE:
      if (kind == SUB_BUTTON) {
        RemovePropW(h, L"QuillHover");
        InvalidateRect(h, NULL, FALSE);
      }
      break;
    case WM_NCDESTROY:
      RemovePropW(h, L"QuillHover");
      RemoveWindowSubclass(h, control_proc, id);
      break;
  }
  if (kind == SUB_PROGRESS) {
    switch (msg) {
      case PBM_SETPOS: case PBM_DELTAPOS: case PBM_STEPIT: case PBM_SETRANGE: case PBM_SETRANGE32: {
        LRESULT res = DefSubclassProc(h, msg, wp, lp);
        InvalidateRect(h, NULL, FALSE);
        return res;
      }
    }
  }
  if (kind == SUB_BUTTON || kind == SUB_CHECK || kind == SUB_GROUP) {
    // The stock control draws itself straight to the screen when its state
    // changes; paint over it afterwards.
    switch (msg) {
      case BM_SETCHECK: case BM_SETSTATE: case BM_SETSTYLE: case BM_CLICK:
      case WM_LBUTTONDOWN: case WM_LBUTTONUP: case WM_LBUTTONDBLCLK:
      case WM_KEYDOWN: case WM_KEYUP: case WM_SETFOCUS: case WM_KILLFOCUS:
      case WM_ENABLE: case WM_SETTEXT: case WM_UPDATEUISTATE: case WM_CAPTURECHANGED: {
        LRESULT res = DefSubclassProc(h, msg, wp, lp);
        InvalidateRect(h, NULL, FALSE);
        return res;
      }
    }
  }
  return DefSubclassProc(h, msg, wp, lp);
}

static LRESULT color_reply(HWND control, HDC dc) {
  COLORREF ground = ground_of(control);
  int id = GetDlgCtrlID(control);
  SetTextColor(dc, id == IDC_HEADER_SUB || id == IDC_BRANDING ? INK2 : INK);
  SetBkColor(dc, ground);
  SetBkMode(dc, OPAQUE);
  return (LRESULT)brush(ground);
}

static void theme_children(HWND parent);

static LRESULT CALLBACK window_proc(HWND h, UINT msg, WPARAM wp, LPARAM lp, UINT_PTR id, DWORD_PTR kind) {
  switch (msg) {
    case WM_CTLCOLORSTATIC:
    case WM_CTLCOLORBTN:
    case WM_CTLCOLOREDIT:
    case WM_CTLCOLORLISTBOX:
      if (msg == WM_CTLCOLOREDIT) {
        SetTextColor((HDC)wp, INK);
        SetBkColor((HDC)wp, FIELD);
        return (LRESULT)brush(FIELD);
      }
      return color_reply((HWND)lp, (HDC)wp);
    case WM_CTLCOLORDLG:
      return (LRESULT)brush(kind == SUB_MAIN ? FOOT : BG);
    case WM_ERASEBKGND: {
      RECT r;
      GetClientRect(h, &r);
      if (kind == SUB_MAIN) {
        // The footer is as far below the buttons' tops as they are above
        // the window's bottom edge; the page color fills the rest.
        RECT b;
        HWND cancel = GetDlgItem(h, IDCANCEL);
        GetWindowRect(cancel, &b);
        MapWindowPoints(NULL, h, (POINT *)&b, 2);
        RECT foot = r;
        foot.top = b.top - (r.bottom - b.bottom);
        RECT page = r;
        page.bottom = foot.top;
        FillRect((HDC)wp, &page, brush(BG));
        FillRect((HDC)wp, &foot, brush(FOOT));
      } else {
        FillRect((HDC)wp, &r, brush(BG));
      }
      return 1;
    }
    case WM_NCDESTROY:
      RemoveWindowSubclass(h, window_proc, id);
      break;
  }
  return DefSubclassProc(h, msg, wp, lp);
}

static BOOL subclassed(HWND h) {
  DWORD_PTR data;
  return GetWindowSubclass(h, control_proc, 1, &data) || GetWindowSubclass(h, window_proc, 1, &data);
}

static void theme_control(HWND c) {
  if (subclassed(c)) return;
  WCHAR cls[64];
  GetClassNameW(c, cls, 64);
  LONG style = GetWindowLongW(c, GWL_STYLE);
  if (!lstrcmpiW(cls, L"Button")) {
    LONG type = style & 0xF;
    DWORD_PTR kind = type <= BS_DEFPUSHBUTTON ? SUB_BUTTON
                     : type == BS_GROUPBOX    ? SUB_GROUP
                     : (type == BS_CHECKBOX || type == BS_AUTOCHECKBOX || type == BS_3STATE || type == BS_AUTO3STATE ||
                        type == BS_RADIOBUTTON || type == BS_AUTORADIOBUTTON)
                         ? SUB_CHECK
                         : 0;
    if (kind) {
      SetWindowTheme(c, L"", L"");
      SetWindowSubclass(c, control_proc, 1, kind);
    }
  } else if (!lstrcmpiW(cls, L"Edit")) {
    SetWindowSubclass(c, control_proc, 1, SUB_EDIT);
    SetWindowPos(c, NULL, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
  } else if (!lstrcmpiW(cls, L"msctls_progress32")) {
    SetWindowSubclass(c, control_proc, 1, SUB_PROGRESS);
    LONG ex = GetWindowLongW(c, GWL_EXSTYLE);
    SetWindowLongW(c, GWL_EXSTYLE, ex & ~(WS_EX_CLIENTEDGE | WS_EX_STATICEDGE));
    SetWindowLongW(c, GWL_STYLE, style & ~WS_BORDER);
    SetWindowPos(c, NULL, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
  } else if (!lstrcmpiW(cls, L"SysListView32")) {
    SetWindowTheme(c, L"DarkMode_Explorer", NULL);
    ListView_SetBkColor(c, LOG);
    ListView_SetTextBkColor(c, LOG);
    ListView_SetTextColor(c, INK2);
  } else if (!lstrcmpiW(cls, L"Static")) {
    LONG type = style & SS_TYPEMASK;
    if (type == SS_ETCHEDHORZ || type == SS_ETCHEDVERT || type == SS_ETCHEDFRAME) ShowWindow(c, SW_HIDE);
  } else if (!lstrcmpiW(cls, L"#32770")) {
    SetWindowSubclass(c, window_proc, 1, SUB_PAGE);
    theme_children(c);
    InvalidateRect(c, NULL, TRUE);
  }
}

static void theme_children(HWND parent) {
  for (HWND c = GetWindow(parent, GW_CHILD); c; c = GetWindow(c, GW_HWNDNEXT)) {
    WCHAR cls[64];
    GetClassNameW(c, cls, 64);
    if (!lstrcmpiW(cls, L"#32770") && subclassed(c)) {
      theme_children(c);  // a page that got new controls
      continue;
    }
    theme_control(c);
  }
}

static UINT_PTR __cdecl plugin_callback(enum NSPIM msg) { return 0; }

// quill_theme::Init: dark title bar, footer and the main window's controls.
__declspec(dllexport) void __cdecl Init(HWND parent, int string_size, TCHAR *variables, stack_t **stacktop, extra_parameters *extra) {
  EXDLL_INIT();
  extra->RegisterPluginCallback(g_inst, plugin_callback);  // stay loaded: the subclasses live here
  BOOL dark = TRUE;
  DwmSetWindowAttribute(parent, 20 /* DWMWA_USE_IMMERSIVE_DARK_MODE */, &dark, sizeof dark);
  if (!subclassed(parent)) SetWindowSubclass(parent, window_proc, 1, SUB_MAIN);
  ShowWindow(GetDlgItem(parent, IDC_BRANDING_SHADOW), SW_HIDE);
  // NSIS disables the version text, and Windows embosses disabled text.
  EnableWindow(GetDlgItem(parent, IDC_BRANDING), TRUE);
  ShowWindow(GetDlgItem(parent, IDC_LINE_1), SW_HIDE);
  ShowWindow(GetDlgItem(parent, IDC_LINE_2), SW_HIDE);
  theme_children(parent);
  InvalidateRect(parent, NULL, TRUE);
}

// quill_theme::Page: the page now on screen, and any controls it moved.
__declspec(dllexport) void __cdecl Page(HWND parent, int string_size, TCHAR *variables, stack_t **stacktop, extra_parameters *extra) {
  EXDLL_INIT();
  ShowWindow(GetDlgItem(parent, IDC_BRANDING_SHADOW), SW_HIDE);
  theme_children(parent);
  RedrawWindow(parent, NULL, NULL, RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_FRAME);
}

BOOL WINAPI DllMain(HINSTANCE inst, DWORD reason, LPVOID reserved) {
  g_inst = inst;
  return TRUE;
}
