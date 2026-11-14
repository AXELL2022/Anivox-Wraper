using System;
using System.Runtime.InteropServices;
using Windows.Graphics;

namespace Anivox.WinUI;

/// <summary>
/// Borderless black popup over one monitor that hosts the site WebView2 while the player is in
/// fullscreen. The application window itself is never resized: the WebView2 controller is
/// re-parented here (<c>CoreWebView2Controller.ParentWindow</c>) and back. A window that covers
/// the monitor without caption or frame makes the taskbar step aside.
/// </summary>
internal sealed class PlayerFullscreenWindow : IDisposable
{
    private const string ClassName = "AnivoxPlayerFullscreen";
    private const uint WS_POPUP = 0x80000000, WS_CLIPCHILDREN = 0x02000000;
    private const uint WM_CLOSE = 0x0010, WM_SETFOCUS = 0x0007;
    private const uint SWP_SHOWWINDOW = 0x0040, SWP_FRAMECHANGED = 0x0020;
    private const int SW_HIDE = 0, BLACK_BRUSH = 4, IDC_ARROW = 32512;
    private const uint SWP_NOACTIVATE = 0x0010, LWA_ALPHA = 0x2;
    private const int GWL_EXSTYLE = -20;
    private const long WS_EX_LAYERED = 0x80000;

    [DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
    private static extern IntPtr GetWindowLongPtr(IntPtr hwnd, int index);

    [DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
    private static extern IntPtr SetWindowLongPtr(IntPtr hwnd, int index, IntPtr value);

    [DllImport("user32.dll")]
    private static extern bool SetLayeredWindowAttributes(IntPtr hwnd, uint key, byte alpha, uint flags);

    private delegate IntPtr WndProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WndClassEx
    {
        public uint cbSize;
        public uint style;
        public IntPtr lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        public string? lpszMenuName;
        public string lpszClassName;
        public IntPtr hIconSm;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern ushort RegisterClassEx(ref WndClassEx wc);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(uint exStyle, string className, string title, uint style,
        int x, int y, int width, int height, IntPtr parent, IntPtr menu, IntPtr instance, IntPtr param);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    private static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hwnd, int command);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern bool SetWindowText(IntPtr hwnd, string text);

    [DllImport("user32.dll")]
    private static extern IntPtr LoadCursor(IntPtr instance, IntPtr name);

    [DllImport("gdi32.dll")]
    private static extern IntPtr GetStockObject(int index);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetModuleHandle(string? name);

    private static bool _registered;
    // Kept alive for the lifetime of the class registration.
    private static readonly WndProc Proc = WindowProc;
    private static PlayerFullscreenWindow? _current;

    private IntPtr _hwnd;

    /// <summary>Alt+F4 or another close request: the player should leave fullscreen.</summary>
    public event Action? CloseRequested;

    /// <summary>The popup got keyboard focus: hand it to the WebView2.</summary>
    public event Action? FocusRequested;

    public IntPtr Handle => _hwnd;

    public PlayerFullscreenWindow(IntPtr owner, string title)
    {
        var instance = GetModuleHandle(null);
        if (!_registered)
        {
            var wc = new WndClassEx
            {
                cbSize = (uint)Marshal.SizeOf<WndClassEx>(),
                lpfnWndProc = Marshal.GetFunctionPointerForDelegate(Proc),
                hInstance = instance,
                hCursor = LoadCursor(IntPtr.Zero, (IntPtr)IDC_ARROW),
                hbrBackground = GetStockObject(BLACK_BRUSH),
                lpszClassName = ClassName,
            };
            if (RegisterClassEx(ref wc) == 0)
            {
                throw new InvalidOperationException($"RegisterClassEx failed: {Marshal.GetLastWin32Error()}");
            }
            _registered = true;
        }
        // Owned by the main window: stays above it, has no taskbar button of its own.
        _hwnd = CreateWindowEx(0, ClassName, title, WS_POPUP | WS_CLIPCHILDREN, 0, 0, 0, 0, owner, IntPtr.Zero, instance, IntPtr.Zero);
        if (_hwnd == IntPtr.Zero)
        {
            throw new InvalidOperationException($"CreateWindowEx failed: {Marshal.GetLastWin32Error()}");
        }
        _current = this;
    }

    /// <summary>
    /// Covers <paramref name="monitor"/> (physical pixels). <paramref name="transparent"/> shows it
    /// fully transparent (layered, alpha 0) until <see cref="Reveal"/>: the WebView2 inside still
    /// draws, so the window can appear only once the site is drawn at the new size.
    /// </summary>
    public void Show(RectInt32 monitor, string title, bool transparent)
    {
        SetWindowText(_hwnd, title);
        SetLayered(transparent, 0);
        SetWindowPos(_hwnd, IntPtr.Zero /* HWND_TOP */, monitor.X, monitor.Y, monitor.Width, monitor.Height,
            SWP_SHOWWINDOW | SWP_FRAMECHANGED | SWP_NOACTIVATE);
    }

    /// <summary>Makes a transparent window opaque again (plain, not layered).</summary>
    public void Reveal() => SetLayered(false, 255);

    /// <summary>Makes the window fully transparent again; the WebView2 inside keeps drawing.</summary>
    public void Conceal() => SetLayered(true, 0);

    public void Activate() => SetForegroundWindow(_hwnd);

    /// <summary>Moves and resizes the window (screen pixels) without activating it.</summary>
    public void MoveTo(RectInt32 rect) =>
        SetWindowPos(_hwnd, IntPtr.Zero /* HWND_TOP */, rect.X, rect.Y, rect.Width, rect.Height, SWP_NOACTIVATE);

    private void SetLayered(bool layered, byte alpha)
    {
        var ex = GetWindowLongPtr(_hwnd, GWL_EXSTYLE).ToInt64();
        var next = layered ? ex | WS_EX_LAYERED : ex & ~WS_EX_LAYERED;
        if (next != ex)
        {
            SetWindowLongPtr(_hwnd, GWL_EXSTYLE, new IntPtr(next));
        }
        if (layered)
        {
            SetLayeredWindowAttributes(_hwnd, 0, alpha, LWA_ALPHA);
        }
    }

    public void Hide() => ShowWindow(_hwnd, SW_HIDE);

    private static IntPtr WindowProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        var window = _current is { } current && current._hwnd == hwnd ? current : null;
        switch (msg)
        {
            case WM_CLOSE when window is not null:
                window.CloseRequested?.Invoke();
                return IntPtr.Zero;
            case WM_SETFOCUS when window is not null:
                window.FocusRequested?.Invoke();
                return IntPtr.Zero;
        }
        return DefWindowProc(hwnd, msg, wParam, lParam);
    }

    public void Dispose()
    {
        if (_hwnd != IntPtr.Zero)
        {
            DestroyWindow(_hwnd);
            _hwnd = IntPtr.Zero;
        }
        if (_current == this)
        {
            _current = null;
        }
    }
}
