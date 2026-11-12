using Microsoft.UI.Xaml;

namespace Anivox.WinUI;

public partial class App : Application
{
    private MainWindow? _window;

    public static App? Instance { get; private set; }

    public App()
    {
        Instance = this;
        InitializeComponent();
    }

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        _window = new MainWindow();
        _window.Activate();
    }

    /// <summary>A second launch was redirected here; may be called on any thread.</summary>
    public void ShowFromActivation()
    {
        var window = _window;
        window?.DispatcherQueue.TryEnqueue(window.ShowFromTray);
    }
}
