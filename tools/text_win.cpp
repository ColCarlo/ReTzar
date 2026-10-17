// «Testi in alta definizione» (an option of ReTzar): Arial Bold from Windows,
// drawn anti-aliased by GDI into an 8-bit cover buffer the size of the text.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdint.h>

static HFONT arial(int height) {
    return CreateFontW(-height, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE, DEFAULT_CHARSET,
                       OUT_TT_PRECIS, CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY,
                       DEFAULT_PITCH | FF_SWISS, L"Arial");
}

// The text's width and the font's cell height, in pixels; 0 on failure.
extern "C" int tz_text_size(const wchar_t* text, int len, int height, int* width,
                            int* cell) {
    HDC dc = CreateCompatibleDC(nullptr);
    if (!dc) return 0;
    HFONT font = arial(height);
    HGDIOBJ old = SelectObject(dc, font);
    SIZE size = {};
    TEXTMETRICW metrics = {};
    int ok = GetTextExtentPoint32W(dc, text, len, &size) && GetTextMetricsW(dc, &metrics);
    SelectObject(dc, old);
    DeleteObject(font);
    DeleteDC(dc);
    if (!ok) return 0;
    *width = size.cx;
    *cell = metrics.tmHeight;
    return 1;
}

// The text in white on black into a `width × cell` buffer of covers.
extern "C" int tz_text_draw(const wchar_t* text, int len, int height, uint8_t* out, int width,
                            int cell) {
    HDC dc = CreateCompatibleDC(nullptr);
    if (!dc) return 0;
    BITMAPINFO info = {};
    info.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    info.bmiHeader.biWidth = width;
    info.bmiHeader.biHeight = -cell;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HBITMAP bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &bits, nullptr, 0);
    if (!bitmap || !bits) {
        DeleteDC(dc);
        return 0;
    }
    HGDIOBJ old_bitmap = SelectObject(dc, bitmap);
    HFONT font = arial(height);
    HGDIOBJ old_font = SelectObject(dc, font);
    uint8_t* pixels = static_cast<uint8_t*>(bits);
    for (int i = 0; i < width * cell * 4; ++i) pixels[i] = 0;
    SetBkMode(dc, TRANSPARENT);
    SetTextColor(dc, RGB(255, 255, 255));
    TextOutW(dc, 0, 0, text, len);
    GdiFlush();
    for (int i = 0; i < width * cell; ++i) out[i] = pixels[i * 4 + 1];
    SelectObject(dc, old_font);
    SelectObject(dc, old_bitmap);
    DeleteObject(font);
    DeleteObject(bitmap);
    DeleteDC(dc);
    return 1;
}
