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
    // The wallet's token holdings (backend.tokenHoldings()); the selector narrows
    // them to this slot's token. The chosen holding id is exposed as selectedHoldingId.
    property var holdings: []
    // The token's definitionId as configured (TOKENS_CONFIG passes it through
    // as-is — base58 or hex). tokenHoldings emits both encodings per holding, so
    // match on whichever this id is: a 64-char hex string filters the holding's
    // definitionIdHex, otherwise the base58 definitionId. (Filtering on a single
    // fixed encoding shows "No funds" whenever the config uses the other one.)
    readonly property string tokenDefinitionId: root.token ? String(root.token.definitionId || "") : ""
    readonly property bool tokenIdIsHex: /^[0-9a-fA-F]{64}$/.test(root.tokenDefinitionId)
    readonly property string selectedHoldingId: accountSelector.selectedAccountId
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
    // objectName forwarded to the account selector, so tests can pick the funding
    // holding for this side deterministically.
    property alias selectorObjectName: accountSelector.objectName

    signal tokenClicked()
    signal inputEdited(string newValue)

    Binding {
        target: tiInput
        property: "text"
        value: root.amount
    }

    radius: 16
    color: root.active ? theme.colors.inputBg : theme.colors.panelBg
    // Height grows with the content: the amount/token row plus the full-width
    // account selector below it (34/20/0px depending on how many holdings match).
    implicitHeight: tiContent.implicitHeight + 28

    Behavior on color { ColorAnimation { duration: 300 } }

    ColumnLayout {
        id: tiContent
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        anchors.topMargin: 14
        spacing: 10

        RowLayout {
            Layout.fillWidth: true
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

            Rectangle {
                id: tokenButton
                Layout.alignment: Qt.AlignVCenter
                height: 40
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
        }

        // Which of the user's holdings for this token to use, full width below the
        // amount/token row. Always shown when there is at least one holding
        // (auto-selecting the single one); "No funds" when there are none.
        ProgramAccountSelector {
            id: accountSelector
            // Half width, pinned to the right edge under the token button.
            Layout.alignment: Qt.AlignRight
            Layout.preferredWidth: Math.round(tiContent.width / 2)
            sourceModel: root.holdings
            accountType: "TokenHolding"
            // Match the holding encoding to the configured id's encoding (see
            // tokenDefinitionId above): hex → definitionIdHex, base58 → definitionId.
            stateField: root.tokenIdIsHex ? "definitionIdHex" : "definitionId"
            stateValue: root.tokenIdIsHex ? root.tokenDefinitionId.toLowerCase() : root.tokenDefinitionId
            selectionMode: ProgramAccountSelector.Input
            showWhenSingle: true
            textAlignment: Text.AlignRight
            backgroundColor: root.theme.colors.panelBg
            hoverColor: root.theme.colors.panelHoverBg
            textColor: root.theme.colors.textPrimary
            secondaryTextColor: root.theme.colors.textSecondary
            borderColor: root.theme.colors.borderStrong
            focusColor: root.theme.colors.ctaBg
        }
    }
}
