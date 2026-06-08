import {
  AlertTriangle,
  ArrowLeft,
  BadgeEuro,
  BookOpen,
  CheckCircle2,
  ClipboardList,
  FileText,
  Layers,
  PanelLeft,
  Printer,
  Receipt,
  Search,
  Sparkles,
  Users,
} from "lucide-react";

import { useT } from "../../i18n";

const sections = [
  { id: "demarrage", title: "Démarrage rapide", icon: Sparkles },
  { id: "liste", title: "Panneau latéral", icon: PanelLeft },
  { id: "creation", title: "Créer une facture", icon: Receipt },
  { id: "brouillons", title: "Brouillons locaux", icon: ClipboardList },
  { id: "tiers-articles", title: "Clients et articles", icon: Users },
  { id: "apercu", title: "Aperçu, modèles et PDF", icon: Printer },
  { id: "statuts", title: "Statuts et actions", icon: FileText },
  { id: "bridge", title: "Parcours comptable bridge", icon: BadgeEuro },
  { id: "exemples", title: "Exemples pratiques", icon: Layers },
  { id: "depannage", title: "Dépannage", icon: AlertTriangle },
];

function DocSection({ id, title, icon: Icon, children }) {
  return (
    <section id={id} className="invoice-doc-section">
      <div className="invoice-doc-section-head">
        <Icon size={18} />
        <h2>{title}</h2>
      </div>
      {children}
    </section>
  );
}

function DocNote({ type = "info", title, children }) {
  const Icon = type === "warning" ? AlertTriangle : CheckCircle2;
  return (
    <div className={`invoice-doc-note ${type}`}>
      <Icon size={16} />
      <div>
        <strong>{title}</strong>
        <p>{children}</p>
      </div>
    </div>
  );
}

function FlowIllustration() {
  return (
    <div className="invoice-doc-flow" aria-label="Flux de traitement d'une facture">
      {[
        ["Brouillon local", "Saisie possible même si tout n'est pas encore synchronisé."],
        ["Document sauvegardé", "La facture existe dans la base active."],
        ["Document bridge", "Les règles comptables sont contrôlées et figées."],
        ["Écriture", "Le lot comptable équilibré peut être préparé."],
      ].map(([title, text], index) => (
        <div key={title} className="invoice-doc-flow-step">
          <span>{index + 1}</span>
          <strong>{title}</strong>
          <p>{text}</p>
        </div>
      ))}
    </div>
  );
}

function TotalsIllustration() {
  return (
    <div className="invoice-doc-total-card">
      <div><span>Prix HT</span><strong>800 000 F CFA</strong></div>
      <div><span>TVA 18%</span><strong>144 000 F CFA</strong></div>
      <div><span>BIC 0%</span><strong>0 F CFA</strong></div>
      <div className="grand"><span>Total TTC</span><strong>944 000 F CFA</strong></div>
    </div>
  );
}

function BridgeIllustration() {
  return (
    <div className="invoice-doc-bridge-map">
      <div><span>Article</span><strong>70615520</strong></div>
      <div><span>Famille</span><strong>STANDARD</strong></div>
      <div><span>Catégorie</span><strong>VENTES</strong></div>
      <div><span>Comptes</span><strong>70615520 / 443000</strong></div>
    </div>
  );
}

function Example({ title, children }) {
  return (
    <article className="invoice-doc-example">
      <strong>{title}</strong>
      <p>{children}</p>
    </article>
  );
}

export default function InvoiceDocumentation({ onBack }) {
  const { t } = useT();

  return (
    <div className="invoice-doc-page">
      <aside className="invoice-doc-nav">
        <div className="invoice-doc-nav-title">
          <BookOpen size={17} />
          <span>Guide facturation</span>
        </div>
        {sections.map(({ id, title, icon: Icon }) => (
          <a key={id} href={`#${id}`}>
            <Icon size={14} />
            {title}
          </a>
        ))}
      </aside>

      <main className="invoice-doc-content">
        <div className="invoice-doc-hero">
          <button type="button" className="btn btn-sm" onClick={onBack}>
            <ArrowLeft size={14} />
            {t("invoice_doc_back")}
          </button>
          <div>
            <span>Documentation utilisateur</span>
            <h1>Facturation et documents</h1>
            <p>
              Ce guide explique le travail quotidien dans la section Facturation : création des documents,
              brouillons locaux, catalogue articles, modèles PDF et parcours comptable bridge.
            </p>
          </div>
        </div>

        <DocSection id="demarrage" title="Démarrage rapide" icon={Sparkles}>
          <p>
            La section Facturation est disponible après connexion à une base. Elle regroupe les factures,
            avoirs, proformas, commandes, articles et règles comptables bridge.
          </p>
          <div className="invoice-doc-grid">
            <Example title="Factures">Documents de vente destinés au client, avec totaux HT, TVA, BIC et TTC.</Example>
            <Example title="Avoirs">Documents de correction ou d'annulation rattachés au cycle commercial.</Example>
            <Example title="Articles">Catalogue utilisé pour remplir rapidement les lignes de facture.</Example>
            <Example title="Comptabilité">Espace bridge pour valider les règles et préparer les écritures.</Example>
          </div>
          <DocNote title="Ordre conseillé">
            Créez ou vérifiez le client, choisissez les articles, sauvegardez la facture, prévisualisez le PDF,
            puis lancez le parcours bridge si la facture doit être comptabilisée.
          </DocNote>
        </DocSection>

        <DocSection id="liste" title="Panneau latéral des documents" icon={PanelLeft}>
          <p>
            Le panneau de gauche liste les documents de l'onglet actif. La recherche filtre par numéro,
            référence ou tiers. Les filtres par statut et période permettent de retrouver rapidement un dossier.
          </p>
          <div className="invoice-doc-side-preview">
            <div className="invoice-doc-search"><Search size={13} /> Rechercher BAGU, FAC-2026...</div>
            <div className="invoice-doc-mini-card active"><strong>Facture FAC-2026-000001</strong><span>BAGU · 12 508 000 F CFA</span></div>
            <div className="invoice-doc-mini-card"><strong>Brouillon local</strong><span>Non synchronisé · XOF</span></div>
          </div>
          <DocNote title="Affichage des brouillons">
            Les brouillons locaux sont affichés avec les documents sauvegardés. Si la base est lente, les brouillons
            restent visibles pendant le chargement des documents distants.
          </DocNote>
        </DocSection>

        <DocSection id="creation" title="Créer une facture" icon={Receipt}>
          <p>
            Le bouton Nouveau crée un document du type sélectionné. Une facture contient un en-tête, un client,
            une devise, des lignes et des conditions de paiement.
          </p>
          <ol className="invoice-doc-steps">
            <li>Choisir l'onglet Factures puis cliquer sur Nouveau.</li>
            <li>Renseigner le client, la date, l'échéance et la référence si elle existe.</li>
            <li>Ajouter les lignes : article, libellé, quantité, unité, prix HT et remise.</li>
            <li>Vérifier les taux TVA/BIC et les totaux calculés automatiquement.</li>
            <li>Enregistrer le brouillon ou enregistrer puis prévisualiser.</li>
          </ol>
          <TotalsIllustration />
          <DocNote title="Calcul des montants">
            Le total ligne HT est calculé après remise de ligne. La remise globale s'applique ensuite sur le total HT,
            puis les taxes sont recalculées proportionnellement.
          </DocNote>
        </DocSection>

        <DocSection id="brouillons" title="Brouillons locaux et synchronisation" icon={ClipboardList}>
          <p>
            Un brouillon local est créé lorsque l'application ne peut pas encore enregistrer le document complet
            dans la base, par exemple si le client manque ou si la connexion ne répond pas.
          </p>
          <FlowIllustration />
          <DocNote type="warning" title="Avant comptabilisation">
            Un document local doit être synchronisé avant d'être comptabilisé. Le parcours bridge peut lancer cette
            synchronisation automatiquement quand les informations minimales sont présentes.
          </DocNote>
        </DocSection>

        <DocSection id="tiers-articles" title="Clients et articles" icon={Users}>
          <p>
            La recherche client interroge la base active. Si le client n'existe pas encore, il peut être créé depuis
            le formulaire. Les articles servent de base aux lignes et portent aussi les informations de mapping bridge.
          </p>
          <div className="invoice-doc-grid">
            <Example title="Client">
              Code, nom, adresse, ville, pays et identifiants fiscaux permettent de compléter l'en-tête.
            </Example>
            <Example title="Article">
              Code, libellé, prix HT, unité, taux de taxe, famille, catégorie et compte de produit.
            </Example>
            <Example title="Compte de produit">
              Un code article comme 70615520 peut être traité comme compte de vente si le profil bridge le confirme.
            </Example>
          </div>
          <DocNote title="Catalogue et règles">
            Le catalogue article accélère la saisie, mais les règles comptables restent contrôlées dans le profil bridge
            afin d'éviter des comptes manquants au moment de la génération de l'écriture.
          </DocNote>
        </DocSection>

        <DocSection id="apercu" title="Aperçu, modèles et PDF" icon={Printer}>
          <p>
            L'aperçu permet de contrôler le rendu avant impression. Le designer de modèles permet d'ajuster les couleurs,
            la typographie, les informations société, le logo, les mentions légales et les conditions de paiement.
          </p>
          <div className="invoice-doc-print-preview">
            <div className="invoice-doc-paper">
              <strong>FACTURE</strong>
              <span>La Baguette du Faso</span>
              <div />
              <p>Total TTC : 12 508 000 F CFA</p>
            </div>
            <div>
              <h3>Export PDF</h3>
              <p>Après validation visuelle, utilisez Export PDF pour produire le fichier client.</p>
            </div>
          </div>
        </DocSection>

        <DocSection id="statuts" title="Statuts et actions" icon={FileText}>
          <p>
            Les statuts indiquent l'étape du document. L'historique garde les changements importants pour faciliter
            le suivi.
          </p>
          <div className="invoice-doc-status-row">
            {["Brouillon", "Proforma", "Facture", "Avoir", "Comptabilisé"].map((status) => (
              <span key={status}>{status}</span>
            ))}
          </div>
          <ul className="invoice-doc-list">
            <li>Modifier : rouvre le formulaire tant que le document n'est pas verrouillé.</li>
            <li>Dupliquer : crée une copie prête à être adaptée.</li>
            <li>Supprimer : disponible principalement sur les brouillons.</li>
            <li>Comptabiliser : disponible après contrôle des règles comptables.</li>
          </ul>
        </DocSection>

        <DocSection id="bridge" title="Parcours comptable bridge" icon={BadgeEuro}>
          <p>
            Le bridge prépare la transformation commerciale vers la comptabilité. Il vérifie le client, les articles,
            le journal, les comptes de produit et les comptes de taxe avant de créer le document bridge.
          </p>
          <BridgeIllustration />
          <ol className="invoice-doc-steps">
            <li>Cliquer sur Créer le document bridge.</li>
            <li>Si des règles manquent, appliquer les règles proposées ou compléter le paramétrage.</li>
            <li>Valider le document bridge.</li>
            <li>Générer l'écriture comptable.</li>
            <li>Préparer le transfert si l'intégration comptable est utilisée.</li>
          </ol>
          <DocNote title="Exemple BAGU">
            Pour BAGU avec les articles 70615520 et 70510000, le système peut proposer le compte collectif 411000,
            les comptes de produit 70615520 et 70510000, la taxe TVA18 et le compte de TVA collectée 443000.
          </DocNote>
        </DocSection>

        <DocSection id="exemples" title="Exemples pratiques" icon={Layers}>
          <div className="invoice-doc-grid">
            <Example title="Facture simple TVA 18%">
              Ligne HT 800 000, TVA 144 000, total TTC 944 000. Le compte produit vient du profil article.
            </Example>
            <Example title="Facture avec remise">
              Une remise de ligne réduit le montant HT de la ligne. Une remise globale réduit le total HT final.
            </Example>
            <Example title="Brouillon local">
              Si la facture est locale, le bridge synchronise d'abord la facture puis relance le contrôle comptable.
            </Example>
            <Example title="Article-compte">
              Un code article 70510000 peut pointer vers le compte de produit 70510000 dans le profil bridge.
            </Example>
          </div>
        </DocSection>

        <DocSection id="depannage" title="Dépannage" icon={AlertTriangle}>
          <div className="invoice-doc-troubleshooting">
            <Example title="Aucun document dans le panneau latéral">
              Vérifiez l'onglet actif, les filtres de statut, la période et la connexion. Les brouillons locaux doivent
              rester visibles même pendant le chargement distant.
            </Example>
            <Example title="Configuration comptable incomplète">
              Ouvrez le parcours bridge, appliquez les règles proposées, puis relancez la création du document bridge.
            </Example>
            <Example title="Compte collectif client introuvable">
              Créez un profil client bridge avec un compte collectif, par exemple 411000.
            </Example>
            <Example title="Journal de vente introuvable">
              Vérifiez que le bridge est initialisé et que le schéma SALE_STANDARD pointe vers un journal actif.
            </Example>
            <Example title="Export PDF impossible">
              Contrôlez d'abord l'aperçu, le modèle sélectionné et le chemin de sauvegarde du fichier.
            </Example>
          </div>
        </DocSection>
      </main>
    </div>
  );
}
