pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import "../components/liquidity"
import "../components/liquidity/AmountMath.js" as AmountMath
import "../components/shared/TokenVisuals.js" as TokenVisuals

// The wallet's liquidity positions. The program has no "list my positions" read
// and an LP definition id can't be reversed back to its pool, so positions are
// discovered the other way round: resolve every pool in AMM_POOLS_CONFIG, then
// match each pool's lpDefinitionId against the wallet's token holdings. A pool
// the config doesn't list therefore can't surface here, however many LP tokens
// the wallet holds for it.
Item {
    id: root

    objectName: "positionsPage"

    // Real backend replica (logos.module("amm_ui")) and the watch runtime,
    // wired from Main.qml. Null until the app is ready.
    property var backend: null
    property var runtime: null

    readonly property int pageMargin: width < 640 ? 16 : 24
    readonly property int contentMaxWidth: 760

    // One entry per pool the wallet holds LP tokens for; see buildPosition().
    property var positions: []
    readonly property int positionCount: root.positions ? root.positions.length : 0

    property bool loading: false
    property string loadError: ""

    // Emitted when a row is activated (click or keyboard); Main.qml opens the
    // pool detail view, from where the pair can be topped up.
    signal positionActivated(var position)

    readonly property bool walletOpen: !!(root.backend && root.backend.isWalletOpen)
    readonly property bool showEmptyState: !root.loading && root.positionCount === 0

    // Monotonic tag for a whole reload: a wallet switch mid-flight would
    // otherwise let the previous wallet's pool resolutions land in the list.
    property int generation: 0
    // Outstanding resolvePoolAccount calls for the current generation.
    property int pendingResolves: 0

    function reload() {
        if (!root.backend || !root.runtime)
            return

        // Reserves move with every swap, so what was resolved last time is stale
        // by the time the page is opened again. Resolving every pool for a page
        // nobody is looking at would be wasted work, so an offscreen reload just
        // marks the list dirty and onVisibleChanged refetches on the way in.
        if (!root.visible)
            return

        const gen = ++root.generation
        root.loading = true
        root.loadError = ""
        root.positions = []
        root.pendingResolves = 0

        // Read isWalletOpen off the backend rather than through the walletOpen
        // binding: this runs from onBackendChanged, where that binding may not
        // have re-evaluated against the new backend yet.
        if (!root.backend.isWalletOpen) {
            // tokenHoldings() needs an open wallet; without one there is nothing
            // to match pools against.
            root.loading = false
            return
        }

        root.runtime.watch(root.backend.poolList(),
            function(pools) {
                if (gen !== root.generation)
                    return
                root.loadHoldings(gen, pools || [])
            },
            function(err) { root.failLoad(gen, "poolList", err) })
    }

    function loadHoldings(gen, pools) {
        root.runtime.watch(root.backend.tokenHoldings(),
            function(holdings) {
                if (gen !== root.generation)
                    return
                root.resolvePools(gen, pools, holdings || [])
            },
            function(err) { root.failLoad(gen, "tokenHoldings", err) })
    }

    // Fans out one resolvePoolAccount per configured pool and collects whichever
    // come back with an LP balance. Order follows completion, not config order.
    function resolvePools(gen, pools, holdings) {
        if (pools.length === 0) {
            root.loading = false
            return
        }

        var collected = []
        root.pendingResolves = pools.length
        for (var i = 0; i < pools.length; ++i)
            root.resolveOne(gen, pools[i], holdings, collected)
    }

    function resolveOne(gen, pool, holdings, collected) {
        const idA = String(pool.tokenADefinitionId || "")
        const idB = String(pool.tokenBDefinitionId || "")
        if (idA.length === 0 || idB.length === 0) {
            // A config entry without definition ids can't be resolved; it simply
            // contributes no position rather than failing the whole list.
            root.finishOne(gen, collected)
            return
        }

        root.runtime.watch(root.backend.resolvePoolAccount(idA, idB),
            function(result) {
                if (gen !== root.generation)
                    return
                var position = root.buildPosition(pool, result, holdings)
                if (position)
                    collected.push(position)
                root.finishOne(gen, collected)
            },
            function(err) {
                if (gen !== root.generation)
                    return
                console.warn("resolvePoolAccount error:", err)
                root.finishOne(gen, collected)
            })
    }

    function finishOne(gen, collected) {
        if (gen !== root.generation)
            return
        root.pendingResolves -= 1
        if (root.pendingResolves > 0)
            return
        root.positions = collected
        root.loading = false
    }

    function failLoad(gen, op, err) {
        if (gen !== root.generation)
            return
        console.warn(op + " error:", err)
        root.loading = false
        root.loadError = qsTr("Failed to load positions: %1").arg(err)
    }

    // Total LP balance the wallet holds for one definition. Summed rather than
    // first-match: nothing stops a wallet holding the same LP token in more than
    // one account, and under-reporting a position would be worse than slow.
    function lpBalanceFor(lpDefinitionId, holdings) {
        var total = "0"
        for (var i = 0; i < holdings.length; ++i) {
            var holding = holdings[i]
            if (String(holding.definitionId || "") === lpDefinitionId)
                total = AmountMath.add(total, String(holding.balanceRaw || "0"))
        }
        return total
    }

    function buildPosition(pool, result, holdings) {
        if (!result || result.status !== "ok")
            return null

        const lpDefinitionId = String(result.lpDefinitionId || "")
        if (lpDefinitionId.length === 0)
            return null

        const supply = String(result.liquiditySupply || "0")
        if (!AmountMath.isUnsigned(supply) || AmountMath.normalize(supply) === "0")
            return null

        const balance = root.lpBalanceFor(lpDefinitionId, holdings)
        if (AmountMath.normalize(balance) === "0")
            return null

        const reserveA = String(result.reserveA || "0")
        const reserveB = String(result.reserveB || "0")

        return {
            // Passed through so a row carries everything the pool detail view
            // would need, without re-reading the config.
            "tokenA": String(pool.tokenA || ""),
            "tokenB": String(pool.tokenB || ""),
            "tokenADefinitionId": String(pool.tokenADefinitionId || ""),
            "tokenBDefinitionId": String(pool.tokenBDefinitionId || ""),
            "poolId": String(result.poolId || pool.poolId || ""),
            "feeBps": result.feeBps !== undefined ? result.feeBps : (Number(pool.feeBps) || 0),
            "lpDefinitionId": lpDefinitionId,
            "lpBalance": balance,
            "liquiditySupply": supply,
            // The wallet's claim on each reserve, floored the same way the
            // program's remove_liquidity floors its payout.
            "amountA": AmountMath.mulDivFloor(reserveA, balance, supply),
            "amountB": AmountMath.mulDivFloor(reserveB, balance, supply),
            "share": (Number(balance) || 0) / (Number(supply) || 1)
        }
    }

    onBackendChanged: root.reload()
    onRuntimeChanged: root.reload()
    // Every entry refetches: a swap on another tab moves the reserves this page's
    // amounts are derived from, and nothing else tells it that happened.
    onVisibleChanged: {
        if (root.visible)
            root.reload()
    }

    Connections {
        target: root.backend
        function onIsWalletOpenChanged() { root.reload() }
    }

    AmmTheme {
        id: theme
    }

    function feeLabel(feeBps) {
        var percentage = Number(feeBps) / 100
        return qsTr("%1%").arg(percentage.toLocaleString(Qt.locale(), "f", 2))
    }

    // Group an exact decimal string without routing it through Number(), which
    // would round any balance past ~15 digits.
    function amountText(rawValue) {
        var digits = String(rawValue).replace(/[^0-9]/g, "").replace(/^0+(?=[0-9])/, "")
        if (digits.length === 0)
            return "0"
        var separator = Qt.locale().groupSeparator
        var grouped = ""
        for (var i = 0; i < digits.length; ++i) {
            if (i > 0 && (digits.length - i) % 3 === 0)
                grouped += separator
            grouped += digits[i]
        }
        return grouped
    }

    // A dust position is still a position: report it as "<0.01%" rather than
    // rounding it to a flat 0% that reads as "you hold nothing".
    function shareText(share) {
        var percent = share * 100
        if (percent > 0 && percent < 0.01)
            return qsTr("<0.01%")
        return qsTr("%1%").arg(percent.toLocaleString(Qt.locale(), "f", 2))
    }

    function emptyStateText() {
        if (root.loadError.length > 0)
            return root.loadError
        if (!root.walletOpen)
            return qsTr("Connect a wallet to see your liquidity positions.")
        return qsTr("No liquidity positions yet. Create a pool or add liquidity to an existing one.")
    }

    Rectangle {
        anchors.fill: parent
        color: theme.colors.background
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, pageContent.y + pageContent.implicitHeight
                                + root.pageMargin)
        flickableDirection: Flickable.VerticalFlick
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
        }

        Item {
            id: pageContent

            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.width < 640 ? 24 : 40
            width: Math.max(0, Math.min(root.contentMaxWidth,
                                        scroll.width - root.pageMargin * 2))
            implicitHeight: contentColumn.implicitHeight

            Column {
                id: contentColumn

                width: parent.width
                spacing: 24

                Column {
                    width: parent.width
                    spacing: 6

                    Text {
                        text: qsTr("Positions")
                        color: theme.colors.textPrimary
                        font.pixelSize: 30
                        font.weight: Font.Bold
                    }

                    Text {
                        width: parent.width
                        text: qsTr("Your share of each pool, from the LP tokens in your wallet.")
                        color: theme.colors.textSecondary
                        font.pixelSize: 13
                        wrapMode: Text.Wrap
                    }
                }

                Rectangle {
                    id: positionsList

                    objectName: "positionsList"
                    width: parent.width
                    implicitHeight: !root.showEmptyState && !root.loading
                                    ? listContent.implicitHeight : 144
                    color: theme.colors.cardBg
                    radius: 16
                    border.color: theme.colors.border
                    border.width: 1

                    Column {
                        id: listContent

                        width: parent.width
                        visible: !root.showEmptyState && !root.loading

                        Item {
                            width: parent.width
                            height: 48

                            Text {
                                anchors.left: parent.left
                                anchors.leftMargin: 20
                                anchors.verticalCenter: parent.verticalCenter
                                text: qsTr("Position")
                                color: theme.colors.textSecondary
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }

                            Text {
                                anchors.right: parent.right
                                anchors.rightMargin: 20
                                anchors.verticalCenter: parent.verticalCenter
                                text: qsTr("Pool share")
                                color: theme.colors.textSecondary
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }
                        }

                        Rectangle {
                            width: parent.width
                            height: 1
                            color: theme.colors.divider
                        }

                        Repeater {
                            model: root.positions || []

                            delegate: PositionRow {
                                width: listContent.width
                                showDivider: index < root.positionCount - 1
                                objectName: "positionRow%1".arg(index)
                            }
                        }

                        Item {
                            width: parent.width
                            height: 8
                        }
                    }

                    Text {
                        id: emptyState

                        objectName: "positionsListEmptyState"
                        anchors.centerIn: parent
                        width: parent.width - 40
                        visible: root.loading || root.showEmptyState
                        text: root.loading ? qsTr("Loading positions…") : root.emptyStateText()
                        color: root.loadError.length > 0 && !root.loading
                               ? theme.colors.error : theme.colors.textSecondary
                        font.pixelSize: 14
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.Wrap
                    }
                }

                // Only pools the app knows about can be matched, so a missing
                // position is a config gap rather than a missing balance.
                Text {
                    width: parent.width
                    visible: root.walletOpen && !root.loading
                    text: qsTr("Positions are matched against the pools in the app's pool config.")
                    color: theme.colors.textPlaceholder
                    font.pixelSize: 11
                    wrapMode: Text.Wrap
                }
            }
        }
    }

    component PositionRow: Item {
        id: row

        required property var modelData
        required property int index
        property bool showDivider: false
        readonly property var position: modelData
        readonly property string pairText: qsTr("%1 / %2")
                                           .arg(String(position.tokenA || ""))
                                           .arg(String(position.tokenB || ""))
        readonly property string feeText: root.feeLabel(position.feeBps)
        readonly property string shareText: root.shareText(position.share)
        readonly property string underlyingText: qsTr("%1 %2 · %3 %4")
                                                 .arg(root.amountText(position.amountA))
                                                 .arg(String(position.tokenA || ""))
                                                 .arg(root.amountText(position.amountB))
                                                 .arg(String(position.tokenB || ""))

        height: 76
        activeFocusOnTab: true

        Accessible.role: Accessible.Button
        Accessible.name: qsTr("%1, %2 fee, %3 of the pool")
                         .arg(row.pairText).arg(row.feeText).arg(row.shareText)
        Accessible.onPressAction: row.activate()

        function activate() {
            root.positionActivated(row.position)
        }

        Keys.onReturnPressed: row.activate()
        Keys.onEnterPressed: row.activate()
        Keys.onSpacePressed: row.activate()

        // Hover/focus tint sits behind the content so the row reads as clickable.
        Rectangle {
            anchors.fill: parent
            color: theme.colors.panelHoverBg
            opacity: rowMouse.containsMouse || row.activeFocus ? 1 : 0
            visible: opacity > 0

            Behavior on opacity {
                NumberAnimation { duration: 120 }
            }
        }

        MouseArea {
            id: rowMouse

            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: row.activate()
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 20
            anchors.rightMargin: 20
            spacing: 12

            Item {
                Layout.preferredWidth: 46
                Layout.preferredHeight: 30
                Accessible.ignored: true

                TokenAvatar {
                    x: 0
                    y: 1
                    symbol: String(row.position.tokenA || "")
                    z: 1
                }

                TokenAvatar {
                    x: 18
                    y: 1
                    symbol: String(row.position.tokenB || "")
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 8

                    Text {
                        text: row.pairText
                        color: theme.colors.textPrimary
                        font.pixelSize: 16
                        font.weight: Font.Medium
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                        Layout.preferredWidth: implicitWidth
                    }

                    Rectangle {
                        Layout.preferredWidth: feePillText.implicitWidth + 16
                        Layout.preferredHeight: 22
                        radius: 6
                        color: theme.colors.inputBg
                        border.color: theme.colors.borderStrong
                        border.width: 1

                        Text {
                            id: feePillText

                            anchors.centerIn: parent
                            text: row.feeText
                            color: theme.colors.textSecondary
                            font.pixelSize: 11
                            font.weight: Font.Medium
                        }
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: row.underlyingText
                    color: theme.colors.textSecondary
                    font.pixelSize: 12
                    elide: Text.ElideRight
                }
            }

            Text {
                text: row.shareText
                color: theme.colors.textPrimary
                font.pixelSize: 14
                font.weight: Font.Medium
                horizontalAlignment: Text.AlignRight
                elide: Text.ElideRight
            }
        }

        Rectangle {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            visible: row.showDivider
            height: 1
            color: theme.colors.divider
        }
    }

    component TokenAvatar: Rectangle {
        id: avatar

        required property string symbol

        width: 28
        height: 28
        radius: 14
        color: TokenVisuals.colorFor(symbol)
        border.color: theme.colors.cardBg
        border.width: 2
        Accessible.ignored: true

        Text {
            anchors.centerIn: parent
            text: TokenVisuals.letterFor(avatar.symbol)
            color: "#FFFFFF"
            font.pixelSize: 10
            font.weight: Font.Bold
            Accessible.ignored: true
        }
    }
}
