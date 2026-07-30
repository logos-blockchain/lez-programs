import QtQuick 2.15
import QtQuick.Layouts 1.15
import Logos.Wallet
import "TokenVisuals.js" as TokenVisuals

Rectangle {
    id: root

    property var theme
    property string label: ""
    property string amount: ""
    property string usdValue: ""
    property var token: null
    property var programAccounts: []
    property int holdingSelectionMode: ProgramAccountSelector.Input
    property bool active: true
    // When true, restrict input to digits only — used for the sell-amount
    // field, whose value is sent to the backend as a raw base-units integer
    // string (see decimalToU128Le in AmmUiBackend.cpp); fractional/decimal
    // input there fails opaquely rather than being scaled.
    property bool digitsOnly: false
    // objectName forwarded to the inner TextInput so UI tests can target the
    // sell/buy amount fields deterministically (see apps/amm/tests/).
    property alias inputObjectName: tiInput.objectName
    // objectName forwarded to the token-select button, so tests can open the
    // right picker by objectId rather than fuzzy text.
    property alias buttonObjectName: tokenButton.objectName

    signal tokenClicked()
    signal inputEdited(string newValue)
    signal holdingSelectionChanged(string accountId, bool createNew)

    property alias selectedHoldingId: holdingSelector.selectedAccountId
    property alias createNewHolding: holdingSelector.createNewSelected
    readonly property bool holdingReady: holdingSelector.ready
    readonly property bool hasHoldingFunds: holdingSelector.hasFunds
    readonly property var selectedHolding: holdingSelector.selectedAccount

    Binding {
        target: tiInput
        property: "text"
        value: root.amount
    }

    radius: 16
    color: root.active ? theme.colors.inputBg : theme.colors.panelBg
    implicitHeight: 110

    Behavior on color { ColorAnimation { duration: 300 } }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.topMargin: 14
        anchors.bottomMargin: 14
        spacing: 8

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4

            Text {
                text: root.label
                color: theme.colors.textSecondary
                font.pixelSize: 14
            }

            Item {
                Layout.fillWidth: true
                height: 44

                TextInput {
                    id: tiInput
                    anchors.fill: parent
                    color: root.active ? theme.colors.textPrimary : theme.colors.textSecondary
                    font.pixelSize: 36
                    font.weight: Font.Bold
                    selectionColor: theme.colors.selection
                    clip: true
                    onTextEdited: {
                        if (root.digitsOnly) {
                            // Amounts are base units (integers) — strip any
                            // character that slips past the validator (e.g.
                            // via paste) before it reaches the backend.
                            var filtered = text.replace(/[^0-9]/g, "")
                            if (filtered !== text)
                                text = filtered // does not re-trigger onTextEdited
                            root.inputEdited(filtered)
                        } else {
                            root.inputEdited(text)
                        }
                    }
                    validator: RegularExpressionValidator {
                        regularExpression: root.digitsOnly ? /^[0-9]*$/ : /^[0-9]*\.?[0-9]*$/
                    }
                }

                Text {
                    anchors.fill: parent
                    text: "0"
                    color: theme.colors.textPlaceholder
                    font: tiInput.font
                    visible: tiInput.text === "" && !tiInput.activeFocus
                    verticalAlignment: Text.AlignVCenter
                }
            }

            Text {
                text: root.usdValue
                color: theme.colors.textSecondary
                font.pixelSize: 13
                visible: root.usdValue !== ""
            }
        }

        ColumnLayout {
            spacing: 6

            Rectangle {
                id: tokenButton
                Layout.alignment: Qt.AlignRight
                Layout.preferredHeight: 40
                radius: 20
                color: tokenBtnHover.containsMouse ? theme.colors.panelHoverBg : theme.colors.panelBg
                implicitWidth: tokenBtnRow.implicitWidth + 24
                Behavior on color { ColorAnimation { duration: 120 } }

            RowLayout {
                id: tokenBtnRow
                anchors.centerIn: parent
                spacing: 6

                Rectangle {
                    width: 24; height: 24; radius: 12
                    color: root.token ? TokenVisuals.colorFor(root.token.symbol) : theme.colors.noTokenCircle
                    visible: root.token !== null
                    Text {
                        anchors.centerIn: parent
                        text: root.token ? TokenVisuals.letterFor(root.token.symbol) : ""
                        color: "#ffffff"
                        font.pixelSize: 10
                        font.weight: Font.Bold
                    }
                }

                Text {
                    text: root.token ? root.token.symbol : "Select token"
                    color: theme.colors.textPrimary
                    font.pixelSize: 15
                    font.weight: root.token ? Font.Medium : Font.Normal
                }

                Text {
                    text: "▼"
                    color: theme.colors.textSecondary
                    font.pixelSize: 10
                }
            }

                MouseArea {
                    id: tokenBtnHover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.tokenClicked()
                }
            }

            ProgramAccountSelector {
                id: holdingSelector

                Layout.alignment: Qt.AlignRight
                Layout.preferredWidth: 190
                sourceModel: root.programAccounts
                accountType: "TokenHolding"
                stateField: "definitionId"
                stateValue: root.token
                            ? String(root.token.definitionIdHex
                                     || root.token.definitionId || "") : ""
                selectionMode: root.holdingSelectionMode
                createNewText: qsTr("Create new TokenHolding")
                placeholderText: root.holdingSelectionMode === ProgramAccountSelector.Input
                                 ? qsTr("Select source") : qsTr("Select destination")
                accessibleName: root.holdingSelectionMode === ProgramAccountSelector.Input
                                ? qsTr("Source TokenHolding for %1").arg(root.label)
                                : qsTr("Destination TokenHolding for %1").arg(root.label)
                backgroundColor: theme.colors.panelBg
                hoverColor: theme.colors.panelHoverBg
                textColor: theme.colors.textPrimary
                secondaryTextColor: theme.colors.textSecondary
                borderColor: theme.colors.borderStrong
                focusColor: theme.colors.ctaBg
                onSelectionChanged: function(accountId, createNew) {
                    root.holdingSelectionChanged(accountId, createNew)
                }
            }
        }
    }
}
