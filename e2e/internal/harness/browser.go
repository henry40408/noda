package harness

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"net/url"
	"os"
	"time"

	"github.com/chromedp/cdproto/cdp"
	"github.com/chromedp/cdproto/emulation"
	"github.com/chromedp/cdproto/input"
	"github.com/chromedp/cdproto/page"
	"github.com/chromedp/chromedp"
	"github.com/chromedp/chromedp/kb"
)

// ScenarioTimeout bounds one scenario, so a wedged Chrome cannot hang the run.
const ScenarioTimeout = 5 * time.Minute

const closeTimeout = 10 * time.Second

// ChromeEnv names the variable that pins the browser binary.
const ChromeEnv = "E2E_CHROME"

// Viewport is the size of the page area, in CSS pixels.
type Viewport struct {
	Width  int64
	Height int64
}

// The default window is a phone's, because `noda web` exists to reach a
// notebook from a phone.
var (
	// Phone is the iPhone 14's viewport.
	Phone = Viewport{Width: 390, Height: 844}
	// Narrow is the narrowest phone still in use.
	Narrow = Viewport{Width: 320, Height: 568}
	// Tablet is the iPad Air in portrait: one pane below 1024px.
	Tablet = Viewport{Width: 834, Height: 1112}
	// Desktop is over the 1024px pane split and under the 1440px margin.
	Desktop = Viewport{Width: 1280, Height: 800}
	// Monitor is wide enough for what points at the note to sit beside it.
	Monitor = Viewport{Width: 1680, Height: 1050}
)

// Browser is a headless Chrome of its own, scoped to one scenario, so that
// a scenario can switch scripts off before it opens its first page.
type Browser struct {
	ctx    context.Context
	cancel context.CancelFunc
	// scripts is whether the page's scripts run. Without them a click cannot
	// be verified, because the recorder that verifies it is a script.
	scripts bool
}

// Open starts a headless Chrome with a phone-sized viewport and the page's
// scripts on or off.
func Open(scripts bool) (*Browser, error) {
	timeoutCtx, cancelTimeout := context.WithTimeout(context.Background(), ScenarioTimeout)
	allocCtx, cancelAlloc := chromedp.NewExecAllocator(timeoutCtx, allocatorOptions()...)
	tabCtx, cancelTab := chromedp.NewContext(allocCtx)
	browser := &Browser{
		scripts: scripts,
		ctx:     tabCtx,
		cancel: func() {
			cancelTab()
			cancelAlloc()
			cancelTimeout()
		},
	}
	if err := chromedp.Do(tabCtx); err != nil {
		browser.cancel()
		return nil, fmt.Errorf("could not start a browser (%s): a local Chromium or Chrome is required, "+
			"or set %s to its path: %w", BrowserChoice(), ChromeEnv, err)
	}
	if err := browser.SetViewport(Phone); err != nil {
		browser.cancel()
		return nil, err
	}
	if !scripts {
		if err := send(browser, emulation.SetScriptExecutionDisabled,
			emulation.SetScriptExecutionDisabledParams{Value: true}); err != nil {
			browser.cancel()
			return nil, err
		}
	}
	return browser, nil
}

// BrowserChoice says which browser Open will start, for the run's log.
func BrowserChoice() string {
	if path := os.Getenv(ChromeEnv); path != "" {
		return ChromeEnv + "=" + path
	}
	return "chromedp's default search (" + ChromeEnv + " is unset)"
}

func allocatorOptions() []chromedp.ExecAllocatorOption {
	options := append([]chromedp.ExecAllocatorOption{}, chromedp.DefaultExecAllocatorOptions[:]...)
	options = append(options,
		chromedp.Flag("hide-scrollbars", true),
		chromedp.WindowSize(int(Phone.Width), int(Phone.Height)),
	)
	// chromedp's own search prefers a chromium on PATH over google-chrome, so
	// CI pins the binary it reports.
	if path := os.Getenv(ChromeEnv); path != "" {
		options = append(options, chromedp.ExecPath(path))
	}
	return options
}

// Close ends the browser, and the profile directory goes with it.
func (b *Browser) Close() {
	closing, cancel := context.WithTimeout(b.ctx, closeTimeout)
	defer cancel()
	_ = chromedp.Cancel(closing)
	b.cancel()
}

func send[P any](b *Browser, command cdp.Command[P, cdp.Empty], params P) error {
	_, err := chromedp.Call(b.ctx, command, params)
	return err
}

// SetViewport resizes the viewport exactly, through CDP because a window size
// would include the platform's window chrome.
func (b *Browser) SetViewport(viewport Viewport) error {
	return send(b, emulation.SetDeviceMetricsOverride, emulation.SetDeviceMetricsOverrideParams{
		Width: viewport.Width, Height: viewport.Height, DeviceScaleFactor: 1, Mobile: true,
	})
}

// PreferScheme emulates prefers-color-scheme, with no stored preference.
func (b *Browser) PreferScheme(scheme string) error {
	return send(b, emulation.SetEmulatedMedia, emulation.SetEmulatedMediaParams{
		Media:    "screen",
		Features: []*emulation.MediaFeature{{Name: "prefers-color-scheme", Value: scheme}},
	})
}

// Goto navigates to the address and waits for the page to load.
func (b *Browser) Goto(address string) error {
	return chromedp.Do(b.ctx, chromedp.Navigate(address))
}

// Back is the browser's own back button.
func (b *Browser) Back() error {
	if err := chromedp.Do(b.ctx, chromedp.NavigateBack()); err != nil {
		return err
	}
	return b.WaitForLoad()
}

// WaitForLoad waits for document.readyState to be "complete". CDP does not
// wait after a click, so a read that follows a navigation waits first.
func (b *Browser) WaitForLoad() error {
	return Eventually("the page to finish loading", func() (bool, error) {
		state, err := Eval[string](b, "document.readyState")
		return state == "complete", err
	})
}

// URL is the page's current URL.
func (b *Browser) URL() (*url.URL, error) {
	location, err := chromedp.Run(b.ctx, chromedp.Location())
	if err != nil {
		return nil, err
	}
	return url.Parse(location)
}

// Do runs chromedp actions, bounded so a stuck one fails instead of waiting
// forever.
func (b *Browser) Do(actions ...chromedp.Action[chromedp.Void]) error {
	bounded, cancel := context.WithTimeout(b.ctx, 10*time.Second)
	defer cancel()
	return chromedp.Do(bounded, actions...)
}

// KeyPress sends one key as a reader's keyboard does: a single keyDown that
// carries the character, then the keyUp. chromedp's own KeyEvent sends the
// character as a separate char event, which Chrome delivers even when the page
// cancelled the keydown, so a shortcut that rightly swallows its key would
// appear to leak it into the field it just focused.
func (b *Browser) KeyPress(key rune) error { return b.KeyPressWith(key, 0) }

// KeyPressWith is KeyPress with modifier keys held. A chord carries no text:
// Ctrl and Enter together type nothing.
func (b *Browser) KeyPressWith(key rune, modifiers kb.Modifier) error {
	events := kb.Encode(key)
	if len(events) == 3 {
		down, char, up := events[0], events[1], events[2]
		down.Text, down.UnmodifiedText = char.Text, char.UnmodifiedText
		events = []*input.DispatchKeyEventParams{down, up}
	}
	for _, event := range events {
		event.Modifiers = int64(modifiers)
		if modifiers != 0 {
			event.Text, event.UnmodifiedText = "", ""
		}
		if err := send(b, input.DispatchKeyEvent, *event); err != nil {
			return err
		}
	}
	return nil
}

// KeyDownDuringComposition is the keydown a browser reports for a key an input
// method is handling: key code 229, with the key it was. Safari sends exactly
// this for the key that ends a composition, and a script that reads only `key`
// takes it for a shortcut.
func (b *Browser) KeyDownDuringComposition(key, code string) error {
	return send(b, input.DispatchKeyEvent, input.DispatchKeyEventParams{
		Type: input.DispatchKeyEventTypeKeyDown, Key: key, Code: code, WindowsVirtualKeyCode: 229,
	})
}

// BeforeEveryPage runs a script in each page this browser opens from now on,
// ahead of the page's own scripts.
func (b *Browser) BeforeEveryPage(source string) error {
	_, err := chromedp.Call(b.ctx, page.AddScriptToEvaluateOnNewDocument,
		page.AddScriptToEvaluateOnNewDocumentParams{Source: source})
	return err
}

// PointerTo moves the mouse, with no button down, so that what answers to
// :hover does.
func (b *Browser) PointerTo(x, y float64) error {
	return send(b, input.DispatchMouseEvent, input.DispatchMouseEventParams{
		Type: input.DispatchMouseEventTypeMouseMoved, X: x, Y: y,
	})
}

// Screenshot writes the viewport as a PNG.
func (b *Browser) Screenshot(path string) error {
	encoded, err := chromedp.Run(b.ctx, chromedp.CaptureScreenshot())
	if err != nil {
		return err
	}
	return os.WriteFile(path, encoded, 0o644)
}

// Pixels is the viewport as drawn, one pixel to a CSS pixel: the viewport's
// device scale factor is 1.
func (b *Browser) Pixels() (image.Image, error) {
	encoded, err := chromedp.Run(b.ctx, chromedp.CaptureScreenshot())
	if err != nil {
		return nil, err
	}
	return png.Decode(bytes.NewReader(encoded))
}

// Eval evaluates a JavaScript expression in the page and decodes the result.
// It also runs with page scripts switched off.
func Eval[T any](b *Browser, expression string) (T, error) {
	return chromedp.Run(b.ctx, chromedp.Evaluate[T](expression))
}

// Exec runs the body of a JavaScript function in the page, with the arguments
// as `arguments`, and decodes what it returns (null decodes to the zero value).
func Exec[T any](b *Browser, body string, arguments ...any) (T, error) {
	var result T
	literal, err := json.Marshal(arguments)
	if err != nil {
		return result, err
	}
	expression := fmt.Sprintf("JSON.stringify((function() {%s}).apply(null, %s)) ?? 'null'", body, literal)
	encoded, err := Eval[string](b, expression)
	if err != nil {
		return result, err
	}
	if err := json.Unmarshal([]byte(encoded), &result); err != nil {
		return result, fmt.Errorf("reading the script's result %s: %w", encoded, err)
	}
	return result, nil
}
