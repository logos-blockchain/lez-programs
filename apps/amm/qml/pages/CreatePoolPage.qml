import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import Logos.Theme
import Logos.Controls

import "../components/pool"

// Two-column pool-creation flow (Uniswap-style): a vertical step rail on the
// left and the active step's panel on the right. Step 1 selects the token pair;
// step 2 enters deposit amounts. Both panels stay alive so the entered values
// persist when navigating between steps via the rail.
Item {
    id: root

    readonly property int pageMargin: 24
    // Breathing room below the navbar. The Trade page fully centers its card
    // (gap = leftover space / 2); this uses a quarter of the leftover so it sits
    // roughly half as far down, scaling with the window, with a sensible floor.
    readonly property int topMargin: Math.max(48, Math.round((scroll.height - content.implicitHeight) / 4))
    readonly property int contentWidth: 760

    // 0x123456…cdef style truncation for showing token addresses compactly.
    function truncated(addr) {
        const a = (addr || "").trim()
        return a.length > 13 ? (a.substring(0, 6) + "…" + a.substring(a.length - 4)) : a
    }

    // Numbers only (digits + a single decimal point) for the deposit amounts.
    RegularExpressionValidator {
        id: amountValidator
        regularExpression: /^[0-9]*\.?[0-9]*$/
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.palette.background
    }

    Flickable {
        id: scroll
        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, content.implicitHeight + root.topMargin + root.pageMargin)
        flickableDirection: Flickable.VerticalFlick

        RowLayout {
            id: content
            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.topMargin
            width: Math.min(scroll.width - root.pageMargin * 2, root.contentWidth)
            spacing: Theme.spacing.xxlarge

            PoolStepRail {
                id: rail
                currentStep: 0
                Layout.preferredWidth: 240
                Layout.alignment: Qt.AlignTop
                // Jump back to an already-reached step (e.g. step 1 to re-pick
                // tokens). Selections persist because both panels stay alive.
                onStepClicked: (index) => { rail.currentStep = index }
            }

            // ── Right: active step's panel (both kept alive to preserve state) ──
            Item {
                id: rightPane
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop
                Layout.preferredHeight: rail.currentStep === 0
                                        ? selectCard.implicitHeight
                                        : depositCard.implicitHeight

                // ── Step 1: select pair ──────────────────────────────────
                Rectangle {
                    id: selectCard
                    width: parent.width
                    visible: rail.currentStep === 0
                    implicitHeight: selectCol.implicitHeight + Theme.spacing.large * 2
                    radius: Theme.spacing.radiusLarge
                    color: Theme.palette.backgroundSecondary
                    border.width: 1
                    border.color: Theme.palette.borderSecondary

                    ColumnLayout {
                        id: selectCol
                        anchors.fill: parent
                        anchors.margins: Theme.spacing.large
                        spacing: Theme.spacing.medium

                        LogosText {
                            text: qsTr("Select pair")
                            font.pixelSize: Theme.typography.panelTitleText
                            font.weight: Theme.typography.weightBold
                            color: Theme.palette.text
                        }
                        LogosText {
                            Layout.fillWidth: true
                            text: qsTr("Choose the two tokens for your pool by entering each token's address.")
                            wrapMode: Text.WordWrap
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }

                        LogosText {
                            Layout.topMargin: Theme.spacing.small
                            text: qsTr("Token A address")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }
                        LogosTextField {
                            id: tokenAField
                            Layout.fillWidth: true
                            placeholderText: "0x…"
                        }

                        LogosText {
                            Layout.topMargin: Theme.spacing.small
                            text: qsTr("Token B address")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }
                        LogosTextField {
                            id: tokenBField
                            Layout.fillWidth: true
                            placeholderText: "0x…"
                        }

                        LogosButton {
                            Layout.fillWidth: true
                            Layout.topMargin: Theme.spacing.medium
                            height: 44
                            text: qsTr("Continue")
                            enabled: tokenAField.text.trim().length > 0
                                     && tokenBField.text.trim().length > 0
                                     && tokenAField.text.trim() !== tokenBField.text.trim()
                            onClicked: rail.currentStep = 1
                        }
                    }
                }

                // ── Step 2: deposit amounts ──────────────────────────────
                Rectangle {
                    id: depositCard
                    width: parent.width
                    visible: rail.currentStep === 1
                    implicitHeight: depositCol.implicitHeight + Theme.spacing.large * 2
                    radius: Theme.spacing.radiusLarge
                    color: Theme.palette.backgroundSecondary
                    border.width: 1
                    border.color: Theme.palette.borderSecondary

                    ColumnLayout {
                        id: depositCol
                        anchors.fill: parent
                        anchors.margins: Theme.spacing.large
                        spacing: Theme.spacing.medium

                        LogosText {
                            text: qsTr("Deposit amounts")
                            font.pixelSize: Theme.typography.panelTitleText
                            font.weight: Theme.typography.weightBold
                            color: Theme.palette.text
                        }
                        LogosText {
                            Layout.fillWidth: true
                            text: qsTr("Set the initial liquidity for the pool.")
                            wrapMode: Text.WordWrap
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }

                        // Token pair carried over from step 1.
                        Rectangle {
                            Layout.fillWidth: true
                            Layout.topMargin: Theme.spacing.small
                            implicitHeight: pairCol.implicitHeight + Theme.spacing.medium * 2
                            radius: Theme.spacing.radiusLarge
                            color: Theme.palette.backgroundTertiary
                            border.width: 1
                            border.color: Theme.palette.borderSecondary

                            ColumnLayout {
                                id: pairCol
                                anchors.fill: parent
                                anchors.margins: Theme.spacing.medium
                                spacing: Theme.spacing.small

                                LogosText {
                                    text: qsTr("Selected pair")
                                    font.pixelSize: Theme.typography.secondaryText
                                    color: Theme.palette.textSecondary
                                }
                                RowLayout {
                                    Layout.fillWidth: true
                                    LogosText {
                                        text: qsTr("Token A")
                                        font.pixelSize: Theme.typography.secondaryText
                                        color: Theme.palette.textSecondary
                                    }
                                    Item { Layout.fillWidth: true }
                                    LogosText {
                                        text: root.truncated(tokenAField.text)
                                        font.pixelSize: Theme.typography.secondaryText
                                        color: Theme.palette.text
                                    }
                                }
                                RowLayout {
                                    Layout.fillWidth: true
                                    LogosText {
                                        text: qsTr("Token B")
                                        font.pixelSize: Theme.typography.secondaryText
                                        color: Theme.palette.textSecondary
                                    }
                                    Item { Layout.fillWidth: true }
                                    LogosText {
                                        text: root.truncated(tokenBField.text)
                                        font.pixelSize: Theme.typography.secondaryText
                                        color: Theme.palette.text
                                    }
                                }
                            }
                        }

                        // Amount inputs, one per token.
                        LogosText {
                            Layout.topMargin: Theme.spacing.small
                            text: qsTr("Token A amount")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }
                        LogosTextField {
                            id: amountAField
                            Layout.fillWidth: true
                            placeholderText: "0.0"
                            Component.onCompleted: textInput.validator = amountValidator
                        }

                        LogosText {
                            Layout.topMargin: Theme.spacing.small
                            text: qsTr("Token B amount")
                            font.pixelSize: Theme.typography.secondaryText
                            color: Theme.palette.textSecondary
                        }
                        LogosTextField {
                            id: amountBField
                            Layout.fillWidth: true
                            placeholderText: "0.0"
                            Component.onCompleted: textInput.validator = amountValidator
                        }

                        LogosButton {
                            Layout.fillWidth: true
                            Layout.topMargin: Theme.spacing.medium
                            height: 44
                            text: qsTr("Create pool")
                            enabled: parseFloat(amountAField.text) > 0
                                     && parseFloat(amountBField.text) > 0
                            // Wiring to the AMM new_definition instruction is a follow-up.
                            onClicked: console.log("create pool",
                                                   tokenAField.text, amountAField.text,
                                                   tokenBField.text, amountBField.text)
                        }
                    }
                }
            }
        }
    }
}
