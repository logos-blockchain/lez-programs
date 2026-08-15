import QtQuick 2.15

import Logos.Theme

import "chrome"
import "pages"
import "state"

Item {
    id: root

    objectName: "tokenApp"

    // Backend replica + account model, bridged from the C++ backend.
    readonly property var backend: logos.module("token_ui")
    readonly property var accountModel: logos.model("token_ui", "accountModel")

    property bool ready: false

    // Fixture data remains available before a wallet is connected. Once the
    // wallet opens, ManagePage replaces it with live token-module reads.
    TokenStore {
        id: tokenStore
    }

    Connections {
        target: logos
        function onViewModuleReadyChanged(moduleName, isReady) {
            if (moduleName === "token_ui")
                root.ready = isReady && root.backend !== null;
        }
    }

    Component.onCompleted: {
        root.ready = root.backend !== null && logos.isViewModuleReady("token_ui");
    }

    // Connectivity banner: shown when a wallet is open but its configured
    // sequencer doesn't answer reachability probes (so transactions will fail).
    Rectangle {
        id: connectionBanner
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        z: 101

        readonly property bool show: root.ready && root.backend && root.backend.isWalletOpen && root.backend.sequencerAddr.length > 0 && !root.backend.sequencerReachable

        height: show ? 32 : 0
        visible: height > 0
        clip: true
        color: Theme.palette.warning

        Behavior on height {
            NumberAnimation {
                duration: 150
                easing.type: Easing.OutCubic
            }
        }

        Text {
            anchors.centerIn: parent
            width: parent.width - 40
            horizontalAlignment: Text.AlignHCenter
            elide: Text.ElideMiddle
            font.pixelSize: 12
            font.weight: Font.Medium
            color: Theme.palette.background
            text: qsTr("Unable to connect to network")
        }
    }

    // The app is usable before wallet connection; writes and live reads become
    // available as soon as the navbar opens a wallet.
    NavBar {
        id: navbar
        anchors.top: connectionBanner.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        z: 100

        backend: root.ready ? root.backend : null
        accountModel: root.accountModel
    }

    Item {
        anchors.top: navbar.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom

        CreatePage {
            anchors.fill: parent
            visible: navbar.currentIndex === 0
            store: tokenStore
            backend: root.ready ? root.backend : null
            runtime: logos

            onRequestInspect: navbar.currentIndex = 1
        }

        ManagePage {
            anchors.fill: parent
            visible: navbar.currentIndex === 1
            store: tokenStore
            backend: root.ready ? root.backend : null
            runtime: logos
        }
    }
}
