import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

import Logos.Theme

import "chrome"
import "pages"

Item {
    id: root

    // Backend replica + account model, bridged from the C++ backend.
    readonly property var backend: logos.module("amm_ui")
    readonly property var accountModel: logos.model("amm_ui", "accountModel")

    property bool ready: false

    // The pool row opened from PoolsPage, or null for the list. There is no
    // StackView here — the Pools tab renders either the list or the detail view.
    property var selectedPool: null

    // Open a pool's detail view on the Explore tab. select() clears selectedPool
    // via onTabChanged, so the pool is assigned after it, not before.
    function openPoolDetail(pool) {
        navbar.select(1, 0)
        root.selectedPool = pool
    }

    // Hand a pool's pair to the Trade tab and leave the detail view.
    function openSwapFor(pool) {
        root.selectedPool = null
        navbar.select(0, 0)
        swapPage.selectPair(pool)
    }

    // Hand a pool's pair to the Pool tab's create/add form and leave the detail view.
    function openLiquidityFor(pool) {
        root.selectedPool = null
        navbar.select(2, 1)
        liquidityPage.selectPair(pool)
    }

    Connections {
        target: logos
        function onViewModuleReadyChanged(moduleName, isReady) {
            if (moduleName === "amm_ui")
                root.ready = isReady && root.backend !== null
        }
    }

    Component.onCompleted: {
        root.ready = root.backend !== null && logos.isViewModuleReady("amm_ui")
    }

    // Connectivity banner: shown when a wallet is open but its configured
    // sequencer doesn't answer reachability probes (so transactions will fail).
    Rectangle {
        id: connectionBanner
        anchors.top:   parent.top
        anchors.left:  parent.left
        anchors.right: parent.right
        z: 101

        readonly property bool show: root.ready
                                     && root.backend
                                     && root.backend.isWalletOpen
                                     && root.backend.sequencerAddr.length > 0
                                     && !root.backend.sequencerReachable

        height: show ? 32 : 0
        visible: height > 0
        clip: true
        color: Theme.palette.warning

        Behavior on height { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }

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

    // The app is always usable; the wallet is opt-in via the navbar "Connect"
    // control. Trade/Liquidity render immediately on launch.
    NavBar {
        id: navbar
        objectName: "navBar"
        anchors.top:   connectionBanner.bottom
        anchors.left:  parent.left
        anchors.right: parent.right
        z: 100

        backend: root.ready ? root.backend : null
        accountModel: root.accountModel

        // Any nav selection leaves the pool detail view behind, so returning to
        // Explore lands on the list rather than on the pool opened last time.
        // Entering the create-pool form clears it for the same reason; a pair
        // handed over by openLiquidityFor() is applied after this runs.
        onTabChanged: {
            root.selectedPool = null
            if (navbar.currentIndex === 2 && navbar.currentSubIndex === 1)
                liquidityPage.resetForm()
        }
    }

    Item {
        anchors.top:    navbar.bottom
        anchors.left:   parent.left
        anchors.right:  parent.right
        anchors.bottom: parent.bottom

        SwapPage {
            id: swapPage

            anchors.fill: parent
            visible: navbar.currentIndex === 0
            backend: root.ready ? root.backend : null
        }

        // Explore tab: the pool list, or the detail view for the pool opened from it.
        PoolsPage {
            anchors.fill: parent
            backend: root.ready ? root.backend : null
            runtime: logos
            visible: navbar.currentIndex === 1 && root.selectedPool === null

            onPoolActivated: function(pool) { root.selectedPool = pool }
        }

        PoolDetailPage {
            anchors.fill: parent
            backend: root.ready ? root.backend : null
            runtime: logos
            visible: navbar.currentIndex === 1 && root.selectedPool !== null
            pool: root.selectedPool

            onBackRequested: root.selectedPool = null
            onSwapRequested: function(pool) { root.openSwapFor(pool) }
            onAddLiquidityRequested: function(pool) { root.openLiquidityFor(pool) }
        }

        // Pool tab, "View positions": the wallet's LP holdings matched to pools.
        PositionsPage {
            anchors.fill: parent
            backend: root.ready ? root.backend : null
            runtime: logos
            visible: navbar.currentIndex === 2 && navbar.currentSubIndex === 0

            onPositionActivated: function(position) { root.openPoolDetail(position) }
        }

        // Pool tab, "Create pool": the new-position / add-liquidity form.
        LiquidityPage {
            id: liquidityPage

            anchors.fill: parent
            backend: root.ready ? root.backend : null
            runtime: logos
            visible: navbar.currentIndex === 2 && navbar.currentSubIndex === 1
        }
    }
}
