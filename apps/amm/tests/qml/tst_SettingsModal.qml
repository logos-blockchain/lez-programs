pragma ComponentBehavior: Bound

import QtQuick
import QtTest

import "../../qml/chrome" as Chrome

TestCase {
    id: testCase

    name: "SettingsModal"

    Component {
        id: backendComponent

        QtObject {
            property string registryUrl: "https://saved.example/registry.json"
            property string registryRequestUrl: "https://override.example/registry.json"
            property string registrySnapshotSource: "remote"
            property string registrySnapshotOrigin: "https://saved.example/registry.json"
            property string registryRefreshState: "failed"
            property string registryRefreshError: "Connection refused"
            property string registryOverrideKind: "env_url"
            property string registryNetworkNote: ""
            property string activeNetwork: ""
            property var networks: []

            function saveRegistryUrl(_url) { }
            function selectNetwork(_id) { }
        }
    }

    Component {
        id: modalComponent

        Chrome.SettingsModal { }
    }

    function test_savedUrlStaysDistinctFromSnapshotAndFailedRequest() {
        var backend = createTemporaryObject(backendComponent, testCase)
        var modal = createTemporaryObject(modalComponent, testCase, { "backend": backend })
        verify(backend)
        verify(modal)
        modal.open()
        var field = findChild(modal, "settingsRegistryUrlField")
        var status = findChild(modal, "settingsRegistryStatus")
        verify(field)
        verify(status)
        compare(field.text, "https://saved.example/registry.json")
        verify(status.text.indexOf("Lists were loaded from https://saved.example/registry.json.") >= 0)
        verify(status.text.indexOf("AMM_REGISTRY_URL overrides the saved URL.") >= 0)
        verify(status.text.indexOf("https://override.example/registry.json") >= 0)
        verify(status.text.indexOf("Connection refused") >= 0)
    }

    function test_explainsHiddenNetworkPicker() {
        var backend = createTemporaryObject(backendComponent, testCase, {
            "registrySnapshotSource": "local",
            "registrySnapshotOrigin": "",
            "registryRequestUrl": "",
            "registryRefreshState": "ready",
            "registryRefreshError": "",
            "registryOverrideKind": "local_files",
            "registryNetworkNote": "Local token files do not declare networks."
        })
        var modal = createTemporaryObject(modalComponent, testCase, { "backend": backend })
        modal.open()
        var status = findChild(modal, "settingsRegistryStatus")
        var selector = findChild(modal, "settingsNetworkSelector")
        verify(status.text.indexOf("local token files") >= 0)
        verify(status.text.indexOf("Local token files do not declare networks.") >= 0)
        verify(selector)
        compare(selector.visible, false)
    }
}
