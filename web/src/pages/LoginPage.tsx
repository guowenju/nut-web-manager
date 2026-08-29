import { zodResolver } from '@hookform/resolvers/zod'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowRight, LoaderCircle, Network } from 'lucide-react'
import { useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { Navigate, useLocation, useNavigate } from 'react-router'
import { z } from 'zod'
import { errorMessage, getSession, login, sessionQueryKey } from '../lib/api.ts'

const loginSchema = z.object({
  username: z.string().trim().min(1, '请输入用户名'),
  password: z.string().min(1, '请输入密码'),
})

type LoginForm = z.infer<typeof loginSchema>

export function LoginPage() {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const location = useLocation()
  const session = useQuery({
    queryKey: sessionQueryKey,
    queryFn: getSession,
    retry: false,
  })
  const {
    register,
    handleSubmit,
    setFocus,
    formState: { errors },
  } = useForm<LoginForm>({
    resolver: zodResolver(loginSchema),
    defaultValues: { username: 'admin', password: '' },
  })
  const loginMutation = useMutation({
    mutationFn: login,
    onSuccess: (authenticated) => {
      queryClient.setQueryData(sessionQueryKey, authenticated)
      const from = (location.state as { from?: string } | null)?.from ?? '/'
      navigate(from, { replace: true })
    },
  })

  useEffect(() => setFocus('password'), [setFocus])

  if (session.data?.authenticated) return <Navigate to="/" replace />

  return (
    <main className="grid min-h-screen place-items-center bg-slate-50 px-5 py-10 text-slate-900">
      <section className="w-full max-w-sm rounded-2xl border border-slate-200 bg-white p-7 shadow-sm">
        <div className="mb-7 flex items-center gap-3">
          <div className="grid size-10 place-items-center rounded-xl bg-emerald-50 text-emerald-600">
            <Network size={22} />
          </div>
          <p className="font-semibold">NUT Web Manager</p>
        </div>

        <form className="space-y-4" onSubmit={handleSubmit((value) => loginMutation.mutate(value))}>
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-slate-700">用户名</span>
              <input
                {...register('username')}
                autoComplete="username"
                className={loginInputClass}
              />
              {errors.username && <span className="mt-1 block text-xs text-rose-600">{errors.username.message}</span>}
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs font-medium text-slate-700">密码</span>
              <input
                {...register('password')}
                type="password"
                autoComplete="current-password"
                className={loginInputClass}
              />
              {errors.password && <span className="mt-1 block text-xs text-rose-600">{errors.password.message}</span>}
            </label>

            {loginMutation.isError && (
              <div className="rounded-xl border border-rose-200 bg-rose-50 px-3.5 py-3 text-sm text-rose-700">
                {errorMessage(loginMutation.error)}
              </div>
            )}

            <button
              type="submit"
              disabled={loginMutation.isPending}
              className="group mt-2 flex w-full items-center justify-center gap-2 rounded-xl bg-emerald-300 px-4 py-3 text-sm font-semibold text-emerald-950 transition hover:bg-emerald-200 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {loginMutation.isPending ? (
                <LoaderCircle size={17} className="animate-spin" />
              ) : (
                <ArrowRight size={17} className="transition-transform group-hover:translate-x-0.5" />
              )}
              登录
            </button>
        </form>
      </section>
    </main>
  )
}

const loginInputClass =
  'w-full rounded-xl border border-slate-200 bg-white px-3.5 py-3 text-sm text-slate-900 outline-none transition placeholder:text-slate-600 focus:border-emerald-500 focus:ring-2 focus:ring-emerald-100'
