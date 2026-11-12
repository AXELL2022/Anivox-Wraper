using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace Anivox.WinUI;

/// <summary>
/// Managed wrapper over <c>anivox_ffi.dll</c> (<c>windows-winui/native</c>), the Rust core
/// shared with the Tauri and GTK builds: site commands, MPV, Discord, session and tray files.
/// </summary>
internal sealed class AnivoxCore : IDisposable
{
    private const string Dll = "anivox_ffi";

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void EventCallback(IntPtr json, IntPtr user);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern IntPtr anivox_create(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string configDir,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string dataDir,
        EventCallback callback,
        IntPtr user);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void anivox_destroy(IntPtr anivox);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void anivox_free(IntPtr text);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int anivox_invoke(
        IntPtr anivox,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string command,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string? argsJson,
        out IntPtr result);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern void anivox_set_page(IntPtr anivox, [MarshalAs(UnmanagedType.LPUTF8Str)] string? uri);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern ulong anivox_begin_load(IntPtr anivox);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int anivox_is_site_url([MarshalAs(UnmanagedType.LPUTF8Str)] string? url);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int anivox_session_load(IntPtr anivox, out IntPtr result);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int anivox_session_save(
        IntPtr anivox,
        [MarshalAs(UnmanagedType.LPUTF8Str)] string sessionJson,
        out IntPtr result);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int anivox_tray_load(IntPtr anivox);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int anivox_tray_save(IntPtr anivox, int enabled, out IntPtr result);

    [DllImport(Dll, CallingConvention = CallingConvention.Cdecl)]
    private static extern int anivox_content_script(
        [MarshalAs(UnmanagedType.LPUTF8Str)] string startupUrl,
        out IntPtr result);

    private readonly EventCallback _callback; // kept alive while Rust holds the pointer
    private IntPtr _handle;

    /// <summary>UI event JSON from the core; raised on a background thread.</summary>
    public event Action<JsonObject>? UiEvent;

    /// <summary>Same folders as the Tauri build (<c>com.yasin.anivox</c>), so settings and history are shared.</summary>
    public static string ConfigDir { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "com.yasin.anivox");

    public static string DataDir { get; } = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "com.yasin.anivox");

    public AnivoxCore()
    {
        _callback = OnEvent;
        _handle = anivox_create(ConfigDir, DataDir, _callback, IntPtr.Zero);
        if (_handle == IntPtr.Zero)
        {
            throw new InvalidOperationException("Не удалось запустить ядро Anivox");
        }
    }

    private void OnEvent(IntPtr json, IntPtr user)
    {
        try
        {
            if (JsonNode.Parse(Marshal.PtrToStringUTF8(json) ?? "") is JsonObject value)
            {
                UiEvent?.Invoke(value);
            }
        }
        catch (Exception)
        {
            // A broken event must never unwind into Rust.
        }
    }

    private static string Take(IntPtr text)
    {
        try
        {
            return Marshal.PtrToStringUTF8(text) ?? "";
        }
        finally
        {
            anivox_free(text);
        }
    }

    /// <summary>Runs a site command. Blocking: call it off the UI thread.</summary>
    public CoreResult Invoke(string command, string? argsJson)
    {
        var code = anivox_invoke(_handle, command, argsJson, out var result);
        return new CoreResult(code == 0, Take(result));
    }

    public void SetPage(string? uri) => anivox_set_page(_handle, uri);

    public ulong BeginLoad() => anivox_begin_load(_handle);

    public static bool IsSiteUrl(string? url) => anivox_is_site_url(url) != 0;

    public JsonObject LoadSession()
    {
        var code = anivox_session_load(_handle, out var result);
        var text = Take(result);
        return code == 0 && JsonNode.Parse(text) is JsonObject value ? value : new JsonObject();
    }

    public CoreResult SaveSession(JsonObject session)
    {
        var code = anivox_session_save(_handle, session.ToJsonString(), out var result);
        return new CoreResult(code == 0, Take(result));
    }

    public bool MinimizeToTray
    {
        get => anivox_tray_load(_handle) != 0;
    }

    public CoreResult SaveMinimizeToTray(bool enabled)
    {
        var code = anivox_tray_save(_handle, enabled ? 1 : 0, out var result);
        return new CoreResult(code == 0, Take(result));
    }

    public static string ContentScript(string startupUrl)
    {
        var code = anivox_content_script(startupUrl, out var result);
        var text = Take(result);
        return code == 0 ? JsonSerializer.Deserialize<string>(text) ?? "" : "";
    }

    public void Dispose()
    {
        if (_handle != IntPtr.Zero)
        {
            anivox_destroy(_handle);
            _handle = IntPtr.Zero;
        }
    }
}

/// <summary>Command outcome: <see cref="Text"/> is a JSON value on success, an error message otherwise.</summary>
internal readonly record struct CoreResult(bool Ok, string Text);
