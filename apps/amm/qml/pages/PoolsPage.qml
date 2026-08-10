pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import "../components/liquidity"
import "../components/swap/TokenVisuals.js" as TokenVisuals

Item {
    id: root

    // Real backend replica (logos.module("amm_ui")) and the watch runtime,
    // wired from Main.qml. Null until the app is ready.
    property var backend: null
    property var runtime: null

    readonly property int pageMargin: width < 640 ? 16 : 24
    readonly property int contentMaxWidth: 760
    readonly property int poolCount: root.pools ? root.pools.length : 0
    readonly property bool showEmptyState: root.poolCount === 0

    // Config-driven known pools, loaded from AmmUiBackend::poolList() (which
    // reads the AMM_POOLS_CONFIG JSON file). Each entry is rendered generically,
    // so adding pairs to the config needs no change here. Empty until the
    // backend is ready and the call resolves.
    property var pools: []

    function loadPools() {
        if (!root.backend || !root.runtime)
            return
        root.runtime.watch(root.backend.poolList(),
            function(list) { root.pools = list },
            function(err) { console.warn("poolList error:", err) })
    }

    onBackendChanged: root.loadPools()
    onRuntimeChanged: root.loadPools()

    AmmTheme {
        id: theme
    }

    function feeLabel(feeBps) {
        var percentage = Number(feeBps) / 100
        return qsTr("%1%").arg(percentage.toLocaleString(Qt.locale(), "f", 2))
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
                        text: qsTr("Pools")
                        color: theme.colors.textPrimary
                        font.pixelSize: 30
                        font.weight: Font.Bold
                    }

                    Text {
                        text: qsTr("Pools")
                        color: theme.colors.textSecondary
                        font.pixelSize: 13
                    }
                }

                Rectangle {
                    id: poolsList

                    objectName: "poolsList"
                    width: parent.width
                    implicitHeight: !root.showEmptyState
                                    ? listContent.implicitHeight : 144
                    color: theme.colors.cardBg
                    radius: 16
                    border.color: theme.colors.border
                    border.width: 1

                    Column {
                        id: listContent

                        width: parent.width
                        visible: !root.showEmptyState

                        Item {
                            width: parent.width
                            height: 48

                            Text {
                                anchors.left: parent.left
                                anchors.leftMargin: 20
                                anchors.verticalCenter: parent.verticalCenter
                                text: qsTr("Pair")
                                color: theme.colors.textSecondary
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }

                            Text {
                                anchors.right: parent.right
                                anchors.rightMargin: 20
                                anchors.verticalCenter: parent.verticalCenter
                                text: qsTr("Fee")
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
                            model: root.pools || []

                            delegate: PoolRow {
                                width: listContent.width
                                showDivider: index < root.poolCount - 1
                                objectName: "poolRow%1".arg(index)
                            }
                        }
                    }

                    Text {
                        id: emptyState

                        objectName: "poolsListEmptyState"
                        anchors.centerIn: parent
                        width: parent.width - 40
                        visible: root.showEmptyState
                        text: qsTr("No pools configured.")
                        color: theme.colors.textSecondary
                        font.pixelSize: 14
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.Wrap
                    }
                }
            }
        }
    }

    component PoolRow: Item {
        id: row

        required property var modelData
        required property int index
        property bool showDivider: false
        readonly property var pool: modelData
        readonly property string pairText: qsTr("%1 / %2")
                                           .arg(String(pool.tokenA || ""))
                                           .arg(String(pool.tokenB || ""))
        readonly property string feeText: root.feeLabel(pool.feeBps)

        height: 68

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
                    symbol: String(row.pool.tokenA || "")
                    z: 1
                }

                TokenAvatar {
                    x: 18
                    y: 1
                    symbol: String(row.pool.tokenB || "")
                }
            }

            Text {
                Layout.fillWidth: true
                text: row.pairText
                color: theme.colors.textPrimary
                font.pixelSize: 16
                font.weight: Font.Medium
                elide: Text.ElideRight
            }

            Rectangle {
                Layout.preferredWidth: feeText.implicitWidth + 20
                Layout.preferredHeight: 30
                radius: 6
                color: theme.colors.inputBg
                border.color: theme.colors.borderStrong
                border.width: 1

                Text {
                    id: feeText

                    anchors.centerIn: parent
                    text: row.feeText
                    color: theme.colors.textPrimary
                    font.pixelSize: 13
                    font.weight: Font.Medium
                }
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
