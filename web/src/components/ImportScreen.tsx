import { useState } from 'react';
import { GitBranch, MessageSquareText, ScanSearch } from 'lucide-react';
import { Button } from './ui/button.tsx';
import { Card, CardContent, CardFooter, CardHeader, CardTitle } from './ui/card.tsx';
import { Input } from './ui/input.tsx';
import { Label } from './ui/label.tsx';
import { Tabs, TabsContent, TabsList, TabsTrigger } from './ui/tabs.tsx';
import { Textarea } from './ui/textarea.tsx';
import { SAMPLE_PGN } from '../ui/samplePgn.ts';
import { t } from '../i18n/index.ts';

export interface ImportScreenProps {
  busy: boolean;
  error: string | null;
  onOpen: (input: { pgn: string } | { fen?: string }) => void;
}

type Tab = 'pgn' | 'position';

/**
 * `POST /api/sessions` — from a pasted PGN, or from a position.
 *
 * The form is one decision with two ways of expressing it, so it is one card:
 * title, the choice as tabs, the field, and the action. The pill-shaped `Tabs`
 * are the design system's; the previous hand-rolled version was the same idea
 * with worse states.
 *
 * The *screen* is a second question, and it used to be answered by accident:
 * the card was dropped 6vh from the top of an otherwise empty full-height page,
 * which is what the old layout happened to do rather than anything anyone
 * chose. This is the first thing anyone sees, and an empty page around a form
 * leaves the obvious question — what is this, and what happens when I press
 * Open — entirely unanswered.
 *
 * So the screen is a pair: the form, and three sentences about what the app
 * does with the game once it has it. They are laid out as two columns of one
 * optical block, centred in the height rather than pinned near the top, with
 * the form on the right where the eye finishes. Below `lg` the prose is dropped
 * rather than stacked: on a narrow screen the form is the whole screen, and
 * three paragraphs above it would only push the thing the user came for below
 * the fold.
 */
export function ImportScreen({ busy, error, onOpen }: ImportScreenProps): React.JSX.Element {
  const [tab, setTab] = useState<Tab>('pgn');
  const [pgn, setPgn] = useState('');
  const [fen, setFen] = useState('');
  const [localError, setLocalError] = useState<string | null>(null);

  const submit = () => {
    setLocalError(null);
    if (tab === 'pgn') {
      if (!pgn.trim()) {
        setLocalError(t('import.emptyPgn'));
        return;
      }
      onOpen({ pgn: pgn.trim() });
    } else {
      onOpen(fen.trim() ? { fen: fen.trim() } : {});
    }
  };

  return (
    <div className="flex min-h-0 flex-1 overflow-y-auto px-5 py-8">
      {/*
        Centred with `m-auto` rather than `justify-center`, which is not a style
        preference: a centred flex child that outgrows its scroll container is
        clipped at the *top*, out of reach of the scrollbar — and this block
        outgrows a short window as soon as the PGN box is on screen. Auto
        margins take the spare room when there is some and none when there is
        not, so the card is centred in a tall window and scrolls from its top
        edge in a short one.

        The measure is held to something the two columns can share: much wider
        and the pair stops reading as one block and becomes two things at
        opposite edges of the window.
      */}
      <div className="m-auto grid w-full max-w-4xl items-center gap-10 lg:grid-cols-[minmax(0,1fr)_minmax(0,28rem)] lg:gap-12">
        <div className="hidden flex-col gap-7 lg:flex">
          <Feature icon={<ScanSearch />} title={t('import.feature.sweep.title')}>
            {t('import.feature.sweep.body')}
          </Feature>
          <Feature icon={<MessageSquareText />} title={t('import.feature.explain.title')}>
            {t('import.feature.explain.body')}
          </Feature>
          <Feature icon={<GitBranch />} title={t('import.feature.branch.title')}>
            {t('import.feature.branch.body')}
          </Feature>
        </div>

        <Card>
          {/* No description under the title: the tabs directly below it say
              "PGN" and "Position", which is the whole of what the sentence
              there used to say. */}
          <CardHeader>
            <CardTitle className="text-xl tracking-tight">{t('import.title')}</CardTitle>
          </CardHeader>

          <CardContent>
            <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)} className="gap-4">
              <TabsList>
                <TabsTrigger value="pgn">{t('import.tab.pgn')}</TabsTrigger>
                <TabsTrigger value="position">{t('import.tab.position')}</TabsTrigger>
              </TabsList>

              <TabsContent value="pgn">
                {/*
                  `field-sizing-fixed` undoes shadcn's default of growing a
                  textarea with its content. A PGN is hundreds of lines; letting
                  the box follow it would push the Open button off the screen the
                  moment anyone pasted a real game.
                */}
                <Textarea
                  value={pgn}
                  spellCheck={false}
                  placeholder={t('import.pgnPlaceholder')}
                  onChange={(event) => setPgn(event.target.value)}
                  className="h-56 resize-y font-mono text-xs leading-relaxed field-sizing-fixed"
                />
              </TabsContent>

              <TabsContent value="position">
                <div className="flex flex-col gap-2">
                  <Label htmlFor="import-fen">{t('import.fenLabel')}</Label>
                  <Input
                    id="import-fen"
                    type="text"
                    value={fen}
                    spellCheck={false}
                    placeholder={t('import.fenPlaceholder')}
                    onChange={(event) => setFen(event.target.value)}
                    className="font-mono text-xs md:text-xs"
                  />
                </div>
              </TabsContent>
            </Tabs>
          </CardContent>

          <CardFooter className="flex-wrap gap-3">
            <Button onClick={submit} disabled={busy}>
              {busy ? t('import.loading') : t('import.submit')}
            </Button>
            {tab === 'pgn' && (
              <Button variant="ghost" onClick={() => setPgn(SAMPLE_PGN)} disabled={busy}>
                {t('import.sample')}
              </Button>
            )}
            {(localError || error) && (
              <span className="text-sm text-destructive">{localError ?? error}</span>
            )}
          </CardFooter>
        </Card>
      </div>
    </div>
  );
}

/**
 * One of the three sentences beside the form.
 *
 * The icon is a marker, not an illustration: it sits in the text's own colour
 * family at the cap height of the title, so the column reads as three
 * paragraphs with a hanging mark rather than as three boxes of chrome. No card,
 * no tinted disc, no border — the form is the only framed thing on this screen,
 * which is what keeps it the thing you look at.
 */
function Feature({
  icon,
  title,
  children,
}: {
  icon: React.ReactNode;
  title: string;
  children: React.ReactNode;
}): React.JSX.Element {
  return (
    <div className="flex gap-3.5">
      <span className="mt-0.5 text-muted-foreground [&_svg]:size-4.5">{icon}</span>
      <div className="flex flex-col gap-1">
        <span className="text-sm font-medium">{title}</span>
        <p className="max-w-prose text-sm leading-relaxed text-muted-foreground">{children}</p>
      </div>
    </div>
  );
}
