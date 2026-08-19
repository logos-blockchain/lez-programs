pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Layouts

import Logos.Controls
import Logos.Theme

Item {
    id: root

    property var store: null
    property int tokenKind: 0
    property int authorityMode: 0
    property bool metadataEnabled: false
    property int metadataStandard: 0
    property bool prepared: false
    property string preparedMessage: ""

    readonly property bool isFungible: tokenKind === 0
    readonly property bool requiresMetadata: !isFungible
    readonly property bool hasMetadata: requiresMetadata || metadataEnabled
    readonly property string maximumU128: "340282366920938463463374607431768211455"
    readonly property string instructionName: isFungible && !hasMetadata ? "new_fungible_definition" : "new_definition_with_metadata"
    readonly property string supplyLabel: isFungible ? qsTr("Initial raw supply") : qsTr("Printable supply")
    readonly property string supplyValue: supplyField.text.length > 0 ? supplyField.text : qsTr("Not set")
    readonly property string authorityValue: !isFungible ? qsTr("Not applicable to NFT definitions") : authorityMode === 0 ? qsTr("None — fixed supply") : authorityMode === 1 ? qsTr("Definition account — self authority") : externalAuthorityField.text.length > 0 ? externalAuthorityField.text : qsTr("External authority required")
    readonly property bool validSupply: isUnsignedU128(supplyField.text)
    readonly property bool validDefinitionTarget: isAccountId(definitionTargetField.text)
    readonly property bool validHoldingTarget: isAccountId(holdingTargetField.text)
    readonly property bool validMetadataTarget: !hasMetadata || isAccountId(metadataTargetField.text)
    readonly property bool validExternalAuthority: !isFungible || authorityMode !== 2 || (isAccountId(externalAuthorityField.text) && externalAuthorityField.text !== "11111111111111111111111111111111")
    readonly property bool canPrepare: validSupply && validDefinitionTarget && validHoldingTarget && validMetadataTarget && validExternalAuthority
    readonly property var authorityOptions: [
        { text: qsTr("Fixed supply — no future minting"), accessibleName: qsTr("Fixed supply with no mint authority") },
        { text: qsTr("Self authority — definition account signs future mints"), accessibleName: qsTr("Use definition account as mint authority") },
        { text: qsTr("External authority — a separate account signs future mints"), accessibleName: qsTr("Use external mint authority") }
    ]
    readonly property var metadataStandards: [
        { text: qsTr("Simple"), accessibleName: qsTr("Use Simple metadata standard") },
        { text: qsTr("Expanded"), accessibleName: qsTr("Use Expanded metadata standard") }
    ]
    readonly property var examplePatterns: [
        { pattern: "fixed", text: qsTr("Fixed example"), accessibleName: qsTr("Load fixed supply creation example") },
        { pattern: "metadata", text: qsTr("Metadata example"), accessibleName: qsTr("Load metadata token creation example") },
        { pattern: "nft", text: qsTr("NFT example"), accessibleName: qsTr("Load non-fungible collection creation example") }
    ]
    readonly property var readinessChecks: [
        { label: qsTr("Definition target"), detail: qsTr("init · signer · writable"), ok: validDefinitionTarget },
        { label: isFungible ? qsTr("Initial holding") : qsTr("NFT master holding"), detail: qsTr("init · signer · writable"), ok: validHoldingTarget },
        { label: qsTr("Metadata target"), detail: hasMetadata ? qsTr("init · signer · writable") : qsTr("not used by this instruction"), ok: hasMetadata ? validMetadataTarget : true }
    ].concat(isFungible && authorityMode === 2 ? [
        { label: qsTr("External mint authority"), detail: qsTr("Must be a non-zero account ID"), ok: validExternalAuthority }
    ] : [])

    component FormTextField: LogosTextField {
        Layout.fillWidth: true
        Layout.preferredHeight: 42
        textInput.selectByMouse: true
    }

    component CompactButton: LogosButton {
        Layout.preferredWidth: 132
        Layout.preferredHeight: 34
        radius: Theme.spacing.radiusMedium
    }

    function isUnsignedU128(value) {
        if (!/^[0-9]+$/.test(value))
            return false;

        var normalized = value.replace(/^0+(?=\d)/, "");
        if (normalized.length < maximumU128.length)
            return true;
        if (normalized.length > maximumU128.length)
            return false;
        return normalized <= maximumU128;
    }

    function isAccountId(value) {
        return /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(value);
    }

    function setPattern(pattern) {
        prepared = false;
        preparedMessage = "";

        if (pattern === "fixed") {
            tokenKind = 0;
            authorityMode = 0;
            metadataEnabled = false;
            nameField.text = qsTr("Example fixed supply");
            supplyField.text = "7654321";
            return;
        }

        if (pattern === "metadata") {
            tokenKind = 0;
            authorityMode = 2;
            metadataEnabled = true;
            metadataStandard = 1;
            nameField.text = qsTr("Example metadata token");
            supplyField.text = "10000000000000000000000000";
            metadataUriField.text = "data:application/json;base64,...";
            creatorsField.text = qsTr("Creator or authority label");
            return;
        }

        tokenKind = 1;
        metadataEnabled = true;
        metadataStandard = 1;
        nameField.text = qsTr("Example collection");
        supplyField.text = "64";
        metadataUriField.text = "data:application/json;base64,...";
        creatorsField.text = qsTr("Master-holding creator");
    }

    function prepareDefinition() {
        if (!canPrepare)
            return;

        var draft = {
            id: "draft-" + Date.now(),
            name: nameField.text.length > 0 ? nameField.text : qsTr("Untitled definition"),
            type: isFungible ? "fungible" : "nonFungible",
            definitionId: definitionTargetField.text,
            holdingId: holdingTargetField.text,
            metadataId: hasMetadata ? metadataTargetField.text : "",
            rawSupply: supplyField.text,
            displaySupply: supplyField.text,
            inferredDecimals: "",
            authorityMode: isFungible ? (authorityMode === 0 ? "fixed" : authorityMode === 1 ? "self" : "external") : "masterHolding",
            authority: isFungible && authorityMode === 2 ? externalAuthorityField.text : authorityMode === 1 ? definitionTargetField.text : "",
            metadataStandard: hasMetadata ? (metadataStandard === 0 ? "Simple" : "Expanded") : "",
            metadataUri: hasMetadata ? metadataUriField.text : "",
            creators: hasMetadata ? creatorsField.text : "",
            description: qsTr("Prepared locally. No token account or transaction was created."),
            source: "draft",
            instruction: instructionName,
            printableCopies: !isFungible ? supplyField.text : ""
        };

        if (store && store.addDraft)
            store.addDraft(draft);

        prepared = true;
        preparedMessage = qsTr("Draft prepared locally. Switch to Inspect to review it alongside the testnet fixtures.");
    }

    Rectangle {
        anchors.fill: parent
        color: "#151515"
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: content.implicitHeight + 48
        flickableDirection: Flickable.VerticalFlick

        ColumnLayout {
            id: content

            width: Math.max(280, Math.min(scroll.width - 40, 1440))
            x: Math.max(20, (scroll.width - width) / 2)
            y: 24
            spacing: 18

            RowLayout {
                Layout.fillWidth: true
                spacing: 18

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 5

                    Text {
                        color: "#E7E1D8"
                        font.pixelSize: 28
                        font.weight: Font.DemiBold
                        text: qsTr("Create token definition")
                    }

                    Text {
                        Layout.fillWidth: true
                        color: "#A9A098"
                        font.pixelSize: 14
                        wrapMode: Text.Wrap
                        text: qsTr("Prepare the exact definition shape first. This prototype does not create accounts, sign, or submit a transaction.")
                    }
                }

                LogosBadge {
                    Layout.alignment: Qt.AlignTop
                    color: Theme.palette.primary
                    text: qsTr("Prototype")
                }
            }

            GridLayout {
                id: workbench

                readonly property int columnCount: content.width >= 1220 ? 3 : content.width >= 820 ? 2 : 1

                Layout.fillWidth: true
                columns: columnCount
                columnSpacing: 14
                rowSpacing: 14

                Rectangle {
                    id: formPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.preferredWidth: workbench.columnCount === 3 ? 520 : 0
                    Layout.columnSpan: workbench.columnCount === 1 ? 1 : 1
                    implicitHeight: formContent.implicitHeight + 32
                    radius: 16
                    color: "#1B1B1B"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: formContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 14

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                            text: qsTr("Definition settings")
                        }

                        LogosTabBar {
                            id: kindTabs

                            Layout.fillWidth: true
                            Layout.preferredHeight: 42
                            currentIndex: root.tokenKind

                            onCurrentIndexChanged: {
                                root.tokenKind = currentIndex;
                                if (root.requiresMetadata)
                                    root.metadataEnabled = true;
                                root.prepared = false;
                            }

                            LogosTabButton {
                                text: qsTr("Fungible")
                                Accessible.name: qsTr("Create fungible token definition")
                            }

                            LogosTabButton {
                                text: qsTr("Non-fungible")
                                Accessible.name: qsTr("Create non-fungible token definition")
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            color: "#A9A098"
                            font.pixelSize: 13
                            wrapMode: Text.Wrap
                            text: root.isFungible ? qsTr("Fungible definitions create one initial holding with the full raw supply.") : qsTr("NFT definitions always include metadata and create a master holding that controls printing.")
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 7

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                text: qsTr("Identity and supply")
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Name")
                            }

                            FormTextField {
                                id: nameField

                                Accessible.name: qsTr("Token definition name")
                                placeholderText: qsTr("Name stored on the definition")
                                text: qsTr("New token")
                                onTextChanged: root.prepared = false
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: root.supplyLabel
                            }

                            FormTextField {
                                id: supplyField

                                Accessible.name: root.supplyLabel
                                placeholderText: qsTr("Unsigned 128-bit integer")
                                text: "1000000"
                                textInput.inputMethodHints: Qt.ImhDigitsOnly
                                textInput.validator: RegularExpressionValidator {
                                    regularExpression: /^[0-9]*$/
                                }
                                onTextChanged: root.prepared = false
                            }

                            Text {
                                Layout.fillWidth: true
                                color: root.validSupply ? "#A9A098" : "#F08A76"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: root.validSupply ? qsTr("Raw u128 value. Decimal display precision is not stored by the Token Program.") : qsTr("Enter an unsigned 128-bit integer (0 through 340282366920938463463374607431768211455).")
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            color: "#303030"
                            visible: root.isFungible
                            Layout.preferredHeight: visible ? 1 : 0
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            visible: root.isFungible
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            spacing: 7

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                text: qsTr("Mint authority")
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("This is the full authority surface at creation. NFTs have no definition-level mint authority.")
                            }

                            Repeater {
                                model: root.authorityOptions

                                delegate: LogosRadioButton {
                                    required property int index
                                    required property var modelData

                                    Layout.fillWidth: true
                                    Accessible.name: modelData.accessibleName
                                    checked: root.authorityMode === index
                                    text: modelData.text
                                    onClicked: {
                                        root.authorityMode = index
                                        root.prepared = false
                                    }
                                }
                            }

                            FormTextField {
                                id: externalAuthorityField

                                Layout.preferredHeight: root.authorityMode === 2 ? 42 : 0
                                visible: root.authorityMode === 2
                                Accessible.name: qsTr("External mint authority account ID")
                                placeholderText: qsTr("External authority account ID")
                                onTextChanged: root.prepared = false
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 10

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 3

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 15
                                        font.weight: Font.DemiBold
                                        text: qsTr("Metadata")
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: root.requiresMetadata ? qsTr("Required for non-fungible definitions") : qsTr("Optional for fungible definitions")
                                    }
                                }

                                LogosSwitch {
                                    id: metadataSwitch

                                    Layout.alignment: Qt.AlignVCenter
                                    Accessible.name: qsTr("Include metadata account")
                                    checked: root.hasMetadata
                                    enabled: !root.requiresMetadata
                                    onToggled: {
                                        root.metadataEnabled = checked;
                                        root.prepared = false;
                                    }
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("The program stores only standard, URI, creators, definition link, and a primary-sale date initialized to 0. It does not validate URI content or metadata schema.")
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.hasMetadata
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 7

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: qsTr("Metadata standard")
                                    }

                                    Repeater {
                                        model: root.metadataStandards

                                        delegate: LogosRadioButton {
                                            required property int index
                                            required property var modelData

                                            Accessible.name: modelData.accessibleName
                                            checked: root.metadataStandard === index
                                            text: modelData.text
                                            onClicked: {
                                                root.metadataStandard = index
                                                root.prepared = false
                                            }
                                        }
                                    }

                                    Item {
                                        Layout.fillWidth: true
                                    }
                                }

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("URI")
                                }

                                FormTextField {
                                    id: metadataUriField

                                    Accessible.name: qsTr("Metadata URI")
                                    placeholderText: qsTr("data:application/json;base64,... or external URI")
                                    onTextChanged: root.prepared = false
                                }

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Creators")
                                }

                                FormTextField {
                                    id: creatorsField

                                    Accessible.name: qsTr("Metadata creators string")
                                    placeholderText: qsTr("Creator account or attribution string")
                                    onTextChanged: root.prepared = false
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 7

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                text: qsTr("New target accounts")
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: qsTr("Every target must be a new default-valued account and authorize this transaction. The prototype can validate shape, not on-chain account state.")
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Definition target account")
                            }

                            FormTextField {
                                id: definitionTargetField

                                Accessible.name: qsTr("Definition target account ID")
                                placeholderText: qsTr("New authorized account ID")
                                onTextChanged: root.prepared = false
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: root.isFungible ? qsTr("Initial fungible holding target") : qsTr("NFT master holding target")
                            }

                            FormTextField {
                                id: holdingTargetField

                                Accessible.name: root.isFungible ? qsTr("Initial fungible holding target account ID") : qsTr("NFT master holding target account ID")
                                placeholderText: qsTr("New authorized account ID")
                                onTextChanged: root.prepared = false
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.hasMetadata
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 7

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Metadata target account")
                                }

                                FormTextField {
                                    id: metadataTargetField

                                    Accessible.name: qsTr("Metadata target account ID")
                                    placeholderText: qsTr("New authorized account ID")
                                    onTextChanged: root.prepared = false
                                }
                            }
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            Repeater {
                                model: root.examplePatterns

                                delegate: CompactButton {
                                    required property var modelData

                                    Accessible.name: modelData.accessibleName
                                    text: modelData.text
                                    onClicked: root.setPattern(modelData.pattern)
                                }
                            }

                            Item {
                                Layout.fillWidth: true
                            }
                        }
                    }
                }

                Rectangle {
                    id: previewPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.preferredWidth: workbench.columnCount === 3 ? 410 : 0
                    Layout.columnSpan: workbench.columnCount === 1 ? 1 : 1
                    implicitHeight: previewContent.implicitHeight + 32
                    radius: 16
                    color: "#1B1B1B"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: previewContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 13

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                            text: qsTr("Definition preview")
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: previewHeadline.implicitHeight + 22
                            radius: 10
                            color: root.isFungible ? "#211914" : "#181D25"
                            border.color: root.isFungible ? "#49301F" : "#31435D"
                            border.width: 1

                            RowLayout {
                                id: previewHeadline

                                anchors.fill: parent
                                anchors.margins: 11
                                spacing: 10

                                Rectangle {
                                    Layout.preferredHeight: 30
                                    Layout.preferredWidth: kindBadge.implicitWidth + 16
                                    radius: 15
                                    color: root.isFungible ? "#2D211A" : "#182534"
                                    border.color: root.isFungible ? "#6A4329" : "#40607A"
                                    border.width: 1

                                    Text {
                                        id: kindBadge

                                        anchors.centerIn: parent
                                        color: root.isFungible ? "#F2D8C7" : "#BFD8F4"
                                        font.pixelSize: 12
                                        font.weight: Font.DemiBold
                                        text: root.isFungible ? qsTr("Fungible") : qsTr("NFT collection")
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideRight
                                    font.pixelSize: 16
                                    font.weight: Font.DemiBold
                                    text: nameField.text.length > 0 ? nameField.text : qsTr("Untitled definition")
                                }
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 9

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: root.isFungible ? qsTr("Stored total supply") : qsTr("Stored printable supply")
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                Text {
                                    color: root.validSupply ? "#E7E1D8" : "#F08A76"
                                    font.pixelSize: 14
                                    font.weight: Font.Medium
                                    text: root.supplyValue
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                color: "#303030"
                            }

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Creation instruction")
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                Text {
                                    color: "#F2D8C7"
                                    font.family: "monospace"
                                    font.pixelSize: 12
                                    text: root.instructionName
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                color: "#303030"
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.isFungible
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 5

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Mint authority")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    wrapMode: Text.Wrap
                                    text: root.authorityValue
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: !root.isFungible
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 5

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Master holding behavior")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    wrapMode: Text.Wrap
                                    text: qsTr("Starts with print balance %1. The master remains reserved, so at most %2 printed copies are possible.").arg(root.supplyValue).arg(root.validSupply && supplyField.text !== "0" ? qsTr("one fewer than the printable supply") : qsTr("none until a positive printable supply is set"))
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: root.hasMetadata ? 1 : 0
                                visible: root.hasMetadata
                                color: "#303030"
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.hasMetadata
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 6

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Metadata account")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 12
                                    text: metadataTargetField.text.length > 0 ? metadataTargetField.text : qsTr("New metadata target required")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    wrapMode: Text.Wrap
                                    text: qsTr("%1 · URI and creators are stored as supplied.").arg(root.metadataStandard === 0 ? qsTr("Simple") : qsTr("Expanded"))
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: holdingOutcome.implicitHeight + 24
                            radius: 10
                            color: "#101010"
                            border.color: "#343434"
                            border.width: 1

                            ColumnLayout {
                                id: holdingOutcome

                                anchors.fill: parent
                                anchors.margins: 12
                                spacing: 6

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: root.isFungible ? qsTr("Initial fungible holding") : qsTr("Initial NFT master holding")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideMiddle
                                    font.family: "monospace"
                                    font.pixelSize: 12
                                    text: holdingTargetField.text.length > 0 ? holdingTargetField.text : qsTr("New holding target required")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    wrapMode: Text.Wrap
                                    text: root.isFungible ? qsTr("Receives the full initial raw supply: %1.").arg(root.supplyValue) : qsTr("Receives the master state and print balance: %1.").arg(root.supplyValue)
                                }
                            }
                        }

                        LogosButton {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 46
                            Accessible.name: qsTr("Prepare token definition draft")
                            enabled: root.canPrepare
                            text: root.canPrepare ? qsTr("Prepare definition") : qsTr("Complete required targets")
                            onClicked: root.prepareDefinition()
                        }

                        Text {
                            Layout.fillWidth: true
                            visible: root.prepared
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            color: "#78C88D"
                            font.pixelSize: 12
                            wrapMode: Text.Wrap
                            text: root.preparedMessage
                        }
                    }
                }

                Rectangle {
                    id: readinessPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.preferredWidth: workbench.columnCount === 3 ? 300 : 0
                    Layout.columnSpan: workbench.columnCount === 1 ? 1 : workbench.columnCount === 2 ? 2 : 1
                    implicitHeight: readinessContent.implicitHeight + 32
                    radius: 16
                    color: "#181818"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: readinessContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 12

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                            text: qsTr("Protocol readiness")
                        }

                        Text {
                            Layout.fillWidth: true
                            color: "#A9A098"
                            font.pixelSize: 13
                            wrapMode: Text.Wrap
                            text: qsTr("Creation requires fresh authorized target accounts. Green checks confirm form shape only; account state remains unverified until a real client reads the chain.")
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        Repeater {
                            model: root.readinessChecks

                            delegate: RowLayout {
                                id: readinessRow

                                required property var modelData

                                Layout.fillWidth: true
                                spacing: 9

                                LogosBadge {
                                    color: readinessRow.modelData.ok ? Theme.palette.success : Theme.palette.error
                                    text: readinessRow.modelData.ok ? qsTr("Ready") : qsTr("Needs input")
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    spacing: 2

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 14
                                        font.weight: Font.Medium
                                        text: readinessRow.modelData.label
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: readinessRow.modelData.detail
                                    }
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: unsupportedNotice.implicitHeight + 22
                            radius: 8
                            color: "#211914"
                            border.color: "#49301F"
                            border.width: 1

                            Text {
                                id: unsupportedNotice

                                anchors.fill: parent
                                anchors.margins: 11
                                color: "#F2D8C7"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: root.hasMetadata ? qsTr("Metadata-backed definitions require typed serialization today; the generic IDL route cannot serialize the structured creation arguments.") : qsTr("This simple fungible path maps to the generic creation instruction, but this prototype never submits it.")
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            color: "#8E8780"
                            font.pixelSize: 12
                            wrapMode: Text.Wrap
                            text: qsTr("No on-chain symbol, decimal field, image, royalty, collection, or mutable metadata setting exists in the current token definition schema.")
                        }
                    }
                }
            }
        }
    }
}
