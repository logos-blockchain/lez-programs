pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import "TokenVisuals.js" as TokenVisuals

Popup {
    id: root

    required property var theme
    property var tokens: []
    property string searchText: ""
    property string title: qsTr("Select a token")
    property string searchPlaceholder: qsTr("Search tokens")
    property string popularTitle: qsTr("Popular tokens")
    property string listTitle: qsTr("Tokens by 24H volume")
    property bool allowCustomEntry: false
    property var disabledReasonForCode: function(code) {
        return qsTr("This token is unavailable (%1).").arg(code || "unknown")
    }
    property var detailForToken: function(token) {
        return token && token.balance !== undefined
                ? qsTr("Balance %1").arg(token.balance) : ""
    }
    readonly property var rows: root.filteredTokens()
    readonly property var popularTokens: root.selectableTokens().slice(0, 5)
    readonly property bool compact: root.height < 360
    readonly property bool showPopular: root.popularTokens.length > 0 && root.height >= 440

    signal tokenSelected(var token)
    signal tokenEntered(string value)

    parent: Overlay.overlay
    x: parent ? Math.round((parent.width - width) / 2) : 0
    y: parent ? Math.round((parent.height - height) / 2) : 0
    width: parent && parent.width > 32
           ? Math.max(0, Math.min(480, parent.width - 32)) : 280
    height: parent && parent.height > 32
            ? Math.max(0, Math.min(600, parent.height - 32)) : 360
    padding: root.compact ? 8 : 20
    modal: true
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

    onOpened: {
        root.searchText = ""
        searchField.text = ""
        Qt.callLater(searchField.forceActiveFocus)
    }

    Overlay.modal: Rectangle {
        color: Qt.rgba(0, 0, 0, 0.4)
    }

    background: Rectangle {
        radius: 24
        color: root.theme.colors.cardBg
        border.color: root.theme.colors.border
        border.width: 1

        Behavior on color {
            ColorAnimation { duration: 180 }
        }
    }

    contentItem: ColumnLayout {
        spacing: root.compact ? 8 : 16

        RowLayout {
            Layout.fillWidth: true

            Text {
                Layout.fillWidth: true
                text: root.title
                color: root.theme.colors.textPrimary
                font.pixelSize: 18
                font.weight: Font.Bold
            }

            Button {
                id: closeButton

                Layout.preferredWidth: 32
                Layout.preferredHeight: 32
                text: "\u00D7"
                hoverEnabled: true
                Accessible.name: qsTr("Close token selector")
                ToolTip.visible: hovered
                ToolTip.text: Accessible.name
                onClicked: root.close()

                contentItem: Text {
                    text: closeButton.text
                    color: root.theme.colors.textSecondary
                    font.pixelSize: 16
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }

                background: Rectangle {
                    radius: 16
                    color: closeButton.hovered || closeButton.activeFocus
                           ? root.theme.colors.panelHoverBg
                           : root.theme.colors.panelBg
                }
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: root.compact ? 40 : 48
            radius: 16
            color: root.theme.colors.inputBg
            border.color: searchField.activeFocus
                          ? root.theme.colors.ctaBg
                          : root.theme.colors.border
            border.width: 1

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 14
                spacing: 8

                Text {
                    text: "\u2315"
                    color: root.theme.colors.textSecondary
                    font.pixelSize: 20
                }

                TextField {
                    id: searchField

                    objectName: "tokenSearchField"

                    Layout.fillWidth: true
                    color: root.theme.colors.textPrimary
                    placeholderText: root.searchPlaceholder
                    placeholderTextColor: root.theme.colors.textPlaceholder
                    selectionColor: root.theme.colors.selection
                    selectedTextColor: root.theme.colors.textPrimary
                    font.pixelSize: 15
                    selectByMouse: true
                    background: null

                    onTextEdited: root.searchText = text
                    onAccepted: root.acceptInput(text)
                }
            }
        }

        Text {
            Layout.fillWidth: true
            visible: root.showPopular
            text: root.popularTitle
            color: root.theme.colors.textSecondary
            font.pixelSize: 13
        }

        Flow {
            Layout.fillWidth: true
            visible: root.showPopular
            spacing: 8

            Repeater {
                model: root.popularTokens

                delegate: AmmTokenSelectButton {
                    required property var modelData

                    theme: root.theme
                    hasToken: true
                    tokenColor: root.tokenColor(modelData)
                    tokenLetter: root.tokenLetter(modelData)
                    showIndicator: false
                    text: root.tokenSymbol(modelData).length > 0
                          ? root.tokenSymbol(modelData) : root.tokenName(modelData)
                    maximumTextWidth: 84
                    width: Math.min(140, implicitWidth)
                    onClicked: root.chooseToken(modelData)
                }
            }
        }

        Text {
            Layout.fillWidth: true
            visible: !root.compact
            text: root.listTitle
            color: root.theme.colors.textSecondary
            font.pixelSize: 13
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ListView {
                id: tokenList

                objectName: "tokenList"

                anchors.fill: parent
                clip: true
                spacing: 2
                model: root.rows
                boundsBehavior: Flickable.StopAtBounds
                ScrollIndicator.vertical: ScrollIndicator { }

                delegate: Item {
                    id: tokenRow
                    objectName: "tokenListItem"

                    required property var modelData
                    // Exposed for UI tests to read the row's token (see swap.mjs).
                    readonly property string tokenSymbol: root.tokenSymbol(modelData)
                                                          || root.tokenName(modelData)
                    readonly property bool selectable: root.isSelectable(modelData)
                    readonly property string disabledReason: root.disabledReasonForCode(
                                                                 modelData.code
                                                                 || modelData.status)
                    readonly property bool pointerHovered: rowHover.containsMouse
                    readonly property bool disabledReasonVisible: (pointerHovered || activeFocus)
                                                                   && !selectable

                    width: ListView.view.width
                    height: 56
                    activeFocusOnTab: true
                    Accessible.role: Accessible.Button
                    Accessible.name: root.tokenName(modelData)
                    Accessible.description: selectable
                                            ? root.detailForToken(modelData)
                                            : disabledReason
                    Accessible.onPressAction: tokenRow.activate()
                    Keys.onReturnPressed: function(event) {
                        tokenRow.activate()
                        event.accepted = true
                    }
                    Keys.onEnterPressed: function(event) {
                        tokenRow.activate()
                        event.accepted = true
                    }
                    Keys.onSpacePressed: function(event) {
                        tokenRow.activate()
                        event.accepted = true
                    }

                    Rectangle {
                        anchors.fill: parent
                        radius: 12
                        color: tokenRow.pointerHovered || tokenRow.activeFocus
                               ? root.theme.colors.panelBg : "transparent"

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 8
                            anchors.rightMargin: 8
                            spacing: 12

                            Rectangle {
                                Layout.preferredWidth: 36
                                Layout.preferredHeight: 36
                                radius: 18
                                color: root.tokenColor(tokenRow.modelData)

                                Text {
                                    anchors.centerIn: parent
                                    text: root.tokenLetter(tokenRow.modelData)
                                    color: "#FFFFFF"
                                    font.pixelSize: 14
                                    font.weight: Font.Bold
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2

                                Text {
                                    Layout.fillWidth: true
                                    text: root.tokenName(tokenRow.modelData)
                                    color: tokenRow.selectable
                                           ? root.theme.colors.textPrimary
                                           : root.theme.colors.textPlaceholder
                                    font.pixelSize: 15
                                    elide: Text.ElideRight
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 6

                                    Text {
                                        visible: text.length > 0
                                        text: root.tokenSymbol(tokenRow.modelData)
                                        color: root.theme.colors.textSecondary
                                        font.pixelSize: 12
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: root.shortAddress(root.tokenAddress(tokenRow.modelData))
                                        color: root.theme.colors.textPlaceholder
                                        font.family: "monospace"
                                        font.pixelSize: 11
                                        elide: Text.ElideMiddle
                                    }
                                }
                            }

                            Text {
                                Layout.maximumWidth: 118
                                text: root.detailForToken(tokenRow.modelData)
                                color: root.theme.colors.textSecondary
                                font.pixelSize: 11
                                horizontalAlignment: Text.AlignRight
                                elide: Text.ElideRight
                            }
                        }
                    }

                    MouseArea {
                        id: rowHover

                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: tokenRow.selectable
                                     ? Qt.PointingHandCursor : Qt.ForbiddenCursor
                        onClicked: tokenRow.activate()
                    }

                    ToolTip.visible: tokenRow.disabledReasonVisible
                    ToolTip.text: tokenRow.disabledReason

                    function activate() {
                        if (tokenRow.selectable)
                            root.chooseToken(tokenRow.modelData)
                    }
                }
            }

            Button {
                id: customEntryButton

                anchors.top: parent.top
                anchors.left: parent.left
                anchors.right: parent.right
                visible: root.rows.length === 0
                         && root.allowCustomEntry
                         && root.searchText.trim().length > 0
                implicitHeight: 56
                text: qsTr("Use TokenDefinition address")
                onClicked: root.acceptInput(root.searchText)

                contentItem: Column {
                    leftPadding: 8
                    spacing: 2

                    Text {
                        width: parent.width - 16
                        text: qsTr("Use TokenDefinition address")
                        color: root.theme.colors.textPrimary
                        font.pixelSize: 14
                        font.weight: Font.Medium
                        elide: Text.ElideRight
                    }

                    Text {
                        width: parent.width - 16
                        text: root.searchText
                        color: root.theme.colors.textSecondary
                        font.family: "monospace"
                        font.pixelSize: 11
                        elide: Text.ElideMiddle
                    }
                }

                background: Rectangle {
                    radius: 12
                    color: customEntryButton.hovered || customEntryButton.activeFocus
                           ? root.theme.colors.panelBg : "transparent"
                }
            }

            Text {
                anchors.top: parent.top
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.topMargin: 16
                visible: root.rows.length === 0
                         && (!root.allowCustomEntry
                             || root.searchText.trim().length === 0)
                text: qsTr("No tokens found")
                color: root.theme.colors.textSecondary
                font.pixelSize: 13
                horizontalAlignment: Text.AlignHCenter
            }
        }
    }

    function filteredTokens() {
        var needle = root.searchText.trim().toLowerCase()
        if (needle.length === 0)
            return root.tokens
        var result = []
        for (var i = 0; i < root.tokens.length; ++i) {
            var token = root.tokens[i]
            if (root.tokenName(token).toLowerCase().indexOf(needle) >= 0
                    || root.tokenSymbol(token).toLowerCase().indexOf(needle) >= 0
                    || root.tokenAddress(token).toLowerCase().indexOf(needle) >= 0)
                result.push(token)
        }
        return result
    }

    function selectableTokens() {
        var result = []
        for (var i = 0; i < root.tokens.length; ++i) {
            if (root.isSelectable(root.tokens[i]))
                result.push(root.tokens[i])
        }
        return result
    }

    function acceptInput(value) {
        var entered = String(value || "").trim()
        if (entered.length === 0)
            return
        var lowered = entered.toLowerCase()
        for (var i = 0; i < root.tokens.length; ++i) {
            var token = root.tokens[i]
            if (root.tokenAddress(token).toLowerCase() === lowered
                    || root.tokenName(token).toLowerCase() === lowered
                    || root.tokenSymbol(token).toLowerCase() === lowered) {
                if (root.isSelectable(token))
                    root.chooseToken(token)
                else {
                    root.tokenEntered(root.tokenAddress(token))
                    root.close()
                }
                return
            }
        }
        if (root.rows.length === 1 && root.isSelectable(root.rows[0])) {
            root.chooseToken(root.rows[0])
            return
        }
        if (root.allowCustomEntry) {
            root.tokenEntered(entered)
            root.close()
        }
    }

    function chooseToken(token) {
        if (!root.isSelectable(token))
            return
        root.tokenSelected(token)
        root.close()
    }

    function isSelectable(token) {
        return token && token.selectable !== false
    }

    function tokenName(token) {
        if (!token)
            return qsTr("Unknown token")
        return String(token.name || token.symbol || qsTr("Unknown token"))
    }

    function tokenSymbol(token) {
        return token ? String(token.symbol || "") : ""
    }

    function tokenAddress(token) {
        return token ? String(token.definitionId || token.address || "") : ""
    }

    function tokenColor(token) {
        // Token config carries no color field; derive it from the symbol the
        // same way TokenInput / PoolsPage do (see TokenVisuals.js). Fall back to
        // the name for resolved tokens (e.g. custom ids) that carry no symbol.
        return token ? TokenVisuals.colorFor(root.tokenSymbol(token) || root.tokenName(token))
                     : root.theme.colors.noTokenCircle
    }

    function tokenLetter(token) {
        return token ? TokenVisuals.letterFor(root.tokenSymbol(token) || root.tokenName(token)) : ""
    }

    function shortAddress(value) {
        var text = String(value || "")
        return text.length > 14 ? text.slice(0, 7) + "..." + text.slice(-5) : text
    }
}
