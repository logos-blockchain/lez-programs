pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/chrome" as Chrome

TestCase {
    id: testCase
    name: "SettingsModal"
    when: windowShown
    width: 800
    height: 800

    Component {
        id: viewportComponent
        Item {
            width: 600
            height: 760
        }
    }

    Component {
        id: backendComponent
        QtObject {
            property string registryUrl: "https://saved.example/registry.json"
            property var registryStatus: testCase.status({})
            property var networks: []
            property string activeNetwork: ""
            property string savedUrl: ""
            property string selectedNetwork: ""

            function saveRegistryUrl(url) {
                savedUrl = url
                registryUrl = url
            }
            function selectNetwork(id) {
                selectedNetwork = id
                activeNetwork = id
            }
        }
    }

    Component {
        id: settingsComponent
        Chrome.SettingsModal {}
    }

    function status(changes) {
        return Object.assign({
            source: "none", effectiveUrl: "", snapshotUrl: "", override: "",
            loading: false, error: ""
        }, changes)
    }

    function openSettings(changes, viewportSize) {
        const backend = createTemporaryObject(backendComponent, testCase, changes || {})
        const viewport = createTemporaryObject(viewportComponent, testCase, viewportSize || {})
        const settings = createTemporaryObject(settingsComponent, viewport, {
            parent: viewport, backend: backend
        })
        verify(backend)
        verify(viewport)
        verify(settings)
        settings.open()
        tryCompare(settings, "visible", true)
        waitForPolish(settings.contentItem)
        return { backend: backend, viewport: viewport, settings: settings }
    }

    function item(fixture, name) {
        const child = findChild(fixture.settings.contentItem, name)
        verify(child, name)
        return child
    }

    function test_statusArrivesAfterRemoteObjectInitialization() {
        const fixture = openSettings({ registryStatus: ({}) })
        compare(item(fixture, "settingsRegistrySource").text, "Active source: None")
        verify(!item(fixture, "settingsRegistrySnapshotUrl").visible)
        verify(!item(fixture, "settingsRegistryEffectiveUrl").visible)
        verify(!item(fixture, "settingsRegistryOverride").visible)
        verify(!item(fixture, "settingsRegistryRefreshStatus").visible)
        fixture.backend.registryStatus = status({ source: "local", override: "TOKENS_CONFIG" })
        compare(item(fixture, "settingsRegistrySource").text, "Active source: Local configuration")
        verify(item(fixture, "settingsRegistryOverride").visible)
        compare(item(fixture, "settingsNetworkUnavailable").text,
                "Network selection is unavailable with local configuration.")
    }

    function test_savedUrlRemainsEditableUnderEnvironmentOverride() {
        const fixture = openSettings({ registryStatus: status({
            source: "remote", override: "AMM_REGISTRY_URL",
            effectiveUrl: "https://environment.example/registry.json",
            snapshotUrl: "https://environment.example/registry.json"
        }) })
        const field = item(fixture, "settingsRegistryUrlField")
        compare(field.text, "https://saved.example/registry.json")
        verify(item(fixture, "settingsRegistryOverride").text.indexOf("AMM_REGISTRY_URL") >= 0)
        verify(item(fixture, "settingsRegistryEffectiveUrl").text.indexOf("https://environment.example/") >= 0)
        field.text = "https://new.example/registry.json"
        item(fixture, "settingsRegistrySaveButton").clicked()
        compare(fixture.backend.savedUrl, "https://new.example/registry.json")
        compare(fixture.backend.registryStatus.effectiveUrl, "https://environment.example/registry.json")
        verify(item(fixture, "settingsRegistryEffectiveUrl").text.indexOf("https://environment.example/") >= 0)
    }

    function test_retainedSnapshotKeepsItsOriginDuringFailedRefresh() {
        const fixture = openSettings({
            registryStatus: status({
                source: "cache", effectiveUrl: "https://requested.example/registry.json",
                snapshotUrl: "https://cached.example/registry.json", loading: true
            }),
            networks: [{ id: "testnet", name: "Testnet" }], activeNetwork: "testnet"
        })
        const source = item(fixture, "settingsRegistrySource")
        const snapshot = item(fixture, "settingsRegistrySnapshotUrl")
        const refresh = item(fixture, "settingsRegistryRefreshStatus")
        compare(source.text, "Active source: Cached registry")
        compare(snapshot.text, "Cached from: https://cached.example/registry.json")
        compare(refresh.text, "Refreshing registry. Current data remains in use.")
        fixture.backend.registryStatus = status({
            source: "cache", effectiveUrl: "https://requested.example/registry.json",
            snapshotUrl: "https://cached.example/registry.json", error: "fetch_failed"
        })
        compare(source.text, "Active source: Cached registry")
        compare(snapshot.text, "Cached from: https://cached.example/registry.json")
        compare(refresh.text, "Registry request failed. Current data remains in use.")
        verify(item(fixture, "settingsRegistryEffectiveUrl").text.indexOf("https://requested.example/") >= 0)
        verify(item(fixture, "settingsNetworkSelector").visible)
        verify(!item(fixture, "settingsNetworkUnavailable").visible)
    }

    function test_emptyNetworkExplanation_data() {
        return [
            { tag: "unconfigured", value: {}, source: "None", reason: "No registry is configured." },
            { tag: "local", value: { source: "local", override: "TOKENS_CONFIG, AMM_POOLS_CONFIG" },
              source: "Local configuration", reason: "Network selection is unavailable with local configuration." },
            { tag: "loading", value: { loading: true, effectiveUrl: "https://example.org/registry.json" },
              source: "None", reason: "Networks will appear when a registry loads." },
            { tag: "fetch-failed", value: { error: "fetch_failed", effectiveUrl: "https://example.org/registry.json" },
              source: "None", reason: "No networks are available because the registry could not be loaded." },
            { tag: "invalid", value: { error: "invalid_registry", effectiveUrl: "https://example.org/registry.json" },
              source: "None", reason: "No networks are available because the registry could not be loaded." },
            { tag: "no-networks", value: { source: "remote", error: "no_networks",
                  snapshotUrl: "https://example.org/registry.json", effectiveUrl: "https://example.org/registry.json" },
              source: "Remote registry", reason: "Add networks to the registry or choose another registry URL." }
        ]
    }

    function test_emptyNetworkExplanation(data) {
        const fixture = openSettings({ registryStatus: status(data.value) })
        verify(!item(fixture, "settingsNetworkSelector").visible)
        const explanation = item(fixture, "settingsNetworkUnavailable")
        verify(explanation.visible)
        verify(explanation.text.indexOf(data.reason) >= 0)
        compare(item(fixture, "settingsRegistrySource").text, "Active source: " + data.source)
        compare(item(fixture, "settingsRegistryOverride").visible, !!data.value.override)
        compare(item(fixture, "settingsRegistrySnapshotUrl").visible, !!data.value.snapshotUrl)
        if (data.value.error === "no_networks")
            compare(item(fixture, "settingsRegistryRefreshStatus").text, "Requested registry has no available networks.")
    }

    function test_networkSelectionTracksBackendAndRemainsUsable() {
        const fixture = openSettings({
            registryStatus: status({ source: "remote", snapshotUrl: "https://example.org/registry.json",
                                     effectiveUrl: "https://example.org/registry.json" }),
            networks: [{ id: "testnet", name: "Testnet" }, { id: "mainnet", name: "Mainnet" }],
            activeNetwork: "mainnet"
        })
        const selector = item(fixture, "settingsNetworkSelector")
        compare(selector.currentValue, "mainnet")
        fixture.backend.activeNetwork = "testnet"
        compare(selector.currentValue, "testnet")
        selector.currentIndex = 1
        selector.activated(1)
        compare(fixture.backend.selectedNetwork, "mainnet")
    }

    function test_longUrlsRemainPlainTextAndScrollableInShortViewport() {
        const url = "https://example.org/<b>registry</b>?path=" + "a".repeat(512)
        const fixture = openSettings({ registryStatus: status({
            source: "remote", snapshotUrl: url, effectiveUrl: url,
            override: "AMM_REGISTRY_URL", loading: true
        }) }, { width: 320, height: 300 })
        verify(fixture.settings.x >= 0)
        verify(fixture.settings.y >= 0)
        verify(fixture.settings.x + fixture.settings.width <= fixture.viewport.width)
        verify(fixture.settings.y + fixture.settings.height <= fixture.viewport.height)
        const scroll = item(fixture, "settingsScroll")
        verify(scroll.height > 0)
        verify(scroll.contentHeight > scroll.height)
        for (const name of ["settingsRegistrySnapshotUrl", "settingsRegistryEffectiveUrl"]) {
            const label = item(fixture, name)
            compare(label.textFormat, Text.PlainText)
            verify(label.text.indexOf(url) >= 0)
            verify(label.width <= scroll.availableWidth)
            verify(label.contentWidth <= label.width + 1)
        }
        const close = item(fixture, "settingsCloseButton")
        close.clicked()
        tryCompare(fixture.settings, "visible", false)
    }
}
