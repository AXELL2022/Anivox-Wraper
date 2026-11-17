using System;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.Web.WebView2.Core;

namespace Anivox.WinUI;

/// <summary>
/// Ad blocking in the site webview: every request (iframes included) is checked by
/// <c>anivox_core::adblock</c> (Brave adblock-rust, RU AdList + EasyList) and blocked ones get an
/// empty 403. Page-specific hiding rules are added as a style sheet on DOMContentLoaded. The
/// site itself and top-level navigations are never blocked; the Shikimori overlay is not filtered.
/// </summary>
public sealed partial class MainWindow
{
    private const string AdblockStyleId = "anivox-adblock-style";

    private bool _adblockOn;

    /// <summary>Top-level navigation in progress: its document request is never blocked.</summary>
    private string? _pendingNavigation;

    private void InitAdblock(CoreWebView2 core)
    {
        core.NavigationStarting += (_, args) => _pendingNavigation = args.Uri;
        core.DOMContentLoaded += (_, _) => ApplyHideStyle();
        ApplyAdblock(LoadBlockAdsSetting());
    }

    /// <summary><c>blockAds</c> from <c>settings.json</c>; on when the file is missing or broken.</summary>
    private bool LoadBlockAdsSetting()
    {
        var result = _core.LoadPlayerSettings();
        return !result.Ok
            || JsonNode.Parse(result.Text) is not JsonObject settings
            || settings["blockAds"]?.GetValue<bool>() != false;
    }

    private void ApplyAdblock(bool enabled)
    {
        var core = Core;
        if (core is null || enabled == _adblockOn)
        {
            return;
        }

        _adblockOn = enabled;
        if (enabled)
        {
            _core.StartAdblock();
            core.AddWebResourceRequestedFilter(
                "*", CoreWebView2WebResourceContext.All, CoreWebView2WebResourceRequestSourceKinds.All);
            core.WebResourceRequested += OnAdblockRequest;
        }
        else
        {
            core.WebResourceRequested -= OnAdblockRequest;
            core.RemoveWebResourceRequestedFilter(
                "*", CoreWebView2WebResourceContext.All, CoreWebView2WebResourceRequestSourceKinds.All);
        }
        ApplyHideStyle();
    }

    private void OnAdblockRequest(CoreWebView2 sender, CoreWebView2WebResourceRequestedEventArgs args)
    {
        var uri = args.Request.Uri;
        var kind = args.ResourceContext switch
        {
            CoreWebView2WebResourceContext.Document => uri == _pendingNavigation ? "document" : "subdocument",
            CoreWebView2WebResourceContext.XmlHttpRequest => "xhr",
            var other => other.ToString().ToLowerInvariant(),
        };
        if (_env is not null && _core.ShouldBlock(uri, sender.Source, kind))
        {
            args.Response = _env.CreateWebResourceResponse(null, 403, "Blocked by Anivox", "");
        }
    }

    /// <summary>Adds the page's hiding rules, or removes them when blocking is off.</summary>
    private void ApplyHideStyle()
    {
        var core = Core;
        if (core is null)
        {
            return;
        }

        var css = _adblockOn ? _core.AdblockCss(core.Source) : null;
        var id = JsonSerializer.Serialize(AdblockStyleId);
        var script = css is null
            ? $"document.getElementById({id})?.remove();"
            : $$"""
              (() => {
                let style = document.getElementById({{id}});
                if (!style) {
                  style = document.createElement('style');
                  style.id = {{id}};
                  (document.head || document.documentElement).appendChild(style);
                }
                style.textContent = {{JsonSerializer.Serialize(css)}};
              })();
              """;
        _ = core.ExecuteScriptAsync(script);
    }

    private void OnBlockAdsChanged(object sender, RoutedEventArgs e)
    {
        if (_settingsLoading)
        {
            return;
        }

        var enabled = BlockAds.IsChecked == true;
        var result = _core.UpdatePlayerSettings(settings => settings["blockAds"] = enabled);
        if (!result.Ok)
        {
            SiteStatus.Text = result.Text;
            return;
        }
        ApplyAdblock(enabled);
        SiteStatus.Text = enabled
            ? "Блокировка рекламы включена. Обновите страницу, если реклама уже загрузилась."
            : "Блокировка рекламы выключена.";
    }
}
